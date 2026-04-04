import { formatStructuredErrorText } from './streamError';

type Translator = (key: string, named?: Record<string, unknown>) => string;
type UnknownRecord = Record<string, unknown>;

type WorkflowTerminalItem = {
  item: UnknownRecord;
  status: string;
};

export type AssistantFailureNotice = {
  detail: string;
  comparableDetails: string[];
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

const resolveFailureDetail = (item: UnknownRecord, t: Translator): AssistantFailureNotice => {
  const rawDetail = normalizeText(item.detail ?? item.error ?? item.message);
  const parsedDetail = normalizeText(formatStructuredErrorText(rawDetail, rawDetail));
  const fallbackTitle = normalizeText(item.title);
  const genericTitles = new Set([
    normalizeText(t('chat.workflow.requestFailed')),
    normalizeText(t('chat.workflow.error'))
  ]);
  const detail = parsedDetail || rawDetail || fallbackTitle;
  if (!detail || genericTitles.has(detail)) {
    const fallbackDetail = t('chat.workflow.requestFailedDetail');
    return {
      detail: fallbackDetail,
      comparableDetails: [normalizeText(fallbackDetail)].filter(Boolean)
    };
  }
  return {
    detail: truncateText(detail),
    comparableDetails: Array.from(
      new Set([rawDetail, parsedDetail, fallbackTitle, detail].map((value) => normalizeText(value)).filter(Boolean))
    )
  };
};

const collectComparableTexts = (value: string): string[] => {
  const normalized = normalizeText(value);
  if (!normalized) return [];
  const parsed = normalizeText(formatStructuredErrorText(value, normalized));
  if (!parsed || parsed === normalized) {
    return [normalized];
  }
  return [normalized, parsed];
};

const matchesFailureText = (candidate: string, expected: string): boolean => {
  const candidateTexts = collectComparableTexts(candidate);
  const expectedTexts = collectComparableTexts(expected);
  if (!candidateTexts.length || !expectedTexts.length) return false;
  return candidateTexts.some((value) => expectedTexts.includes(value));
};

const collectFailureNoticeComparableTexts = (notice: AssistantFailureNotice, t: Translator): string[] => {
  const texts = new Set<string>();
  [...notice.comparableDetails, notice.detail].forEach((detail) => {
    const normalizedDetail = normalizeText(detail);
    if (!normalizedDetail) return;
    texts.add(normalizedDetail);
    const reasonLine = normalizeText(t('chat.message.failedInlineReason', { detail }));
    if (reasonLine) {
      texts.add(reasonLine);
    }
  });
  return Array.from(texts);
};

const matchesFailureNoticeText = (candidate: string, comparableTexts: string[]): boolean =>
  comparableTexts.some((expected) => matchesFailureText(candidate, expected));

const sanitizeFailurePartialContent = (
  baseContent: string,
  notice: AssistantFailureNotice,
  t: Translator
): string => {
  const trimmed = baseContent.trim();
  if (!trimmed) return '';

  const comparableTexts = collectFailureNoticeComparableTexts(notice, t);
  if (matchesFailureNoticeText(trimmed, comparableTexts)) {
    return '';
  }

  const normalizedPartialHint = normalizeText(t('chat.message.failedInlinePartial'));
  const isTrailingWrapperLine = (line: string): boolean => {
    const normalizedLine = normalizeText(line);
    if (!normalizedLine) return true;
    if (normalizedLine === normalizedPartialHint || normalizedLine === '---') return true;
    return matchesFailureNoticeText(line, comparableTexts);
  };

  const lines = trimmed.split(/\r?\n/);
  let end = lines.length - 1;
  while (end >= 0 && isTrailingWrapperLine(lines[end])) {
    end -= 1;
  }
  const cleaned = lines.slice(0, end + 1).join('\n').trim();
  if (!cleaned) return '';
  if (matchesFailureNoticeText(cleaned, comparableTexts)) {
    return '';
  }
  return cleaned;
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
  return resolveFailureDetail(terminal.item, t);
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
  const partialContent = sanitizeFailurePartialContent(baseContent, notice, t);
  if (partialContent) {
    prefix.push('', t('chat.message.failedInlinePartial'), '', '---', '', partialContent);
  }
  return prefix.join('\n');
};
