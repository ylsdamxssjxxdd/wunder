export type MessageFeedbackVote = 'up' | 'down';

export type MessageFeedbackState = {
  vote: MessageFeedbackVote;
  created_at?: string;
  locked?: boolean;
};

export const normalizeMessageFeedbackVote = (
  value: unknown
): MessageFeedbackVote | '' => {
  const normalized = String(value || '').trim().toLowerCase();
  if (
    normalized === 'up' ||
    normalized === 'like' ||
    normalized === 'thumb_up' ||
    normalized === 'thumbs_up'
  ) {
    return 'up';
  }
  if (
    normalized === 'down' ||
    normalized === 'dislike' ||
    normalized === 'thumb_down' ||
    normalized === 'thumbs_down'
  ) {
    return 'down';
  }
  return '';
};

export const resolveMessageHistoryId = (message: unknown): number => {
  const raw = (message as Record<string, unknown> | null)?.history_id;
  const parsed = Number.parseInt(String(raw ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
};

export const normalizeMessageFeedback = (
  value: unknown
): MessageFeedbackState | null => {
  if (!value || typeof value !== 'object') {
    return null;
  }
  const record = value as Record<string, unknown>;
  const vote = normalizeMessageFeedbackVote(record.vote);
  if (!vote) {
    return null;
  }
  const createdAtRaw = String(record.created_at ?? record.createdAt ?? '').trim();
  const normalized: MessageFeedbackState = {
    vote,
    locked: record.locked === undefined ? true : record.locked === true
  };
  if (createdAtRaw) {
    normalized.created_at = createdAtRaw;
  }
  return normalized;
};

export const resolveMessageFeedbackVote = (message: unknown): MessageFeedbackVote | '' => {
  const feedback = normalizeMessageFeedback(
    (message as Record<string, unknown> | null)?.feedback
  );
  return feedback?.vote || '';
};

export const isMessageFeedbackLocked = (message: unknown): boolean => {
  const feedback = normalizeMessageFeedback(
    (message as Record<string, unknown> | null)?.feedback
  );
  return Boolean(feedback?.vote);
};

