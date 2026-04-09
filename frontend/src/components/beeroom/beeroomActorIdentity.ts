export type BeeroomActorTranslation = (key: string, params?: Record<string, unknown>) => string;

const PLACEHOLDER_ACTOR_KEYS = new Set(['-', '新会话', 'newsession']);

const normalizeComparableActorKey = (value: unknown): string =>
  String(value || '')
    .trim()
    .toLowerCase()
    .replace(/[\s_-]+/g, '');

export const isBeeroomDefaultAgentLike = (value: unknown): boolean => {
  const raw = String(value || '').trim().toLowerCase();
  if (!raw) return false;
  if (raw === '__default__') return true;
  const normalized = normalizeComparableActorKey(raw);
  return normalized === 'defaultagent' || normalized === '默认智能体';
};

export const normalizeBeeroomActorName = (
  value: unknown,
  t: BeeroomActorTranslation
): string => {
  const text = String(value || '').trim();
  if (!text || PLACEHOLDER_ACTOR_KEYS.has(text.toLowerCase())) {
    return '';
  }
  if (isBeeroomDefaultAgentLike(text)) {
    return t('messenger.defaultAgent');
  }
  return text;
};
