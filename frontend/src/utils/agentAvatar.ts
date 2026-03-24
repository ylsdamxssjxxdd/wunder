import defaultAgentAvatarImage from '@/assets/qq-avatars/avatar-0119.jpg';
import {
  DEFAULT_PROFILE_AVATAR_IMAGE_KEY,
  PROFILE_AVATAR_IMAGE_MAP,
  PROFILE_AVATAR_OPTION_KEYS
} from '@/utils/avatarCatalog';
import { DEFAULT_AVATAR_COLOR, normalizeAvatarColor, normalizeAvatarIcon } from '@/utils/userPreferences';

export type AgentAvatarIconConfig = {
  name: string;
  color: string;
};

const DEFAULT_AGENT_AVATAR_ICON_NAME = DEFAULT_PROFILE_AVATAR_IMAGE_KEY;
const FALLBACK_AGENT_AVATAR_ICON_NAME = 'initial';
const LEGACY_DEFAULT_AVATAR_KEY = 'qq-avatar-0199';

const normalizeLegacyAvatarName = (value: unknown): string => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (!text) return '';
  const directAvatarMatch = text.match(/^avatar-(\d{1,4})$/);
  if (directAvatarMatch) {
    return `qq-avatar-${String(Number.parseInt(directAvatarMatch[1], 10)).padStart(4, '0')}`;
  }
  const qqAvatarMatch = text.match(/^qq-avatar-(\d{1,4})$/);
  if (qqAvatarMatch) {
    const normalized = `qq-avatar-${String(Number.parseInt(qqAvatarMatch[1], 10)).padStart(4, '0')}`;
    return normalized === LEGACY_DEFAULT_AVATAR_KEY ? DEFAULT_AGENT_AVATAR_ICON_NAME : normalized;
  }
  if (text === 'default') {
    return DEFAULT_AGENT_AVATAR_ICON_NAME;
  }
  return text;
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};

const tryParseJsonRecord = (value: unknown): Record<string, unknown> | null => {
  if (typeof value !== 'string') return null;
  const text = value.trim();
  if (!text || !text.startsWith('{')) return null;
  try {
    const parsed = JSON.parse(text);
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
};

type ExtractedIconFields = {
  rawName: string;
  rawColor: string;
  hasExplicitName: boolean;
};

const extractAgentIconFields = (value: unknown): ExtractedIconFields => {
  if (typeof value === 'string') {
    const text = value.trim();
    if (!text) {
      return { rawName: '', rawColor: '', hasExplicitName: false };
    }
    const parsed = tryParseJsonRecord(text);
    if (!parsed) {
      return { rawName: text, rawColor: '', hasExplicitName: true };
    }
    const rawName = String(parsed.name ?? parsed.icon ?? parsed.avatar_icon ?? parsed.avatarIcon ?? '').trim();
    const rawColor = String(parsed.color ?? parsed.avatar_color ?? parsed.avatarColor ?? '').trim();
    return { rawName, rawColor, hasExplicitName: Boolean(rawName) };
  }
  const record = asRecord(value);
  const rawName = String(record.name ?? record.icon ?? record.avatar_icon ?? record.avatarIcon ?? '').trim();
  const rawColor = String(record.color ?? record.avatar_color ?? record.avatarColor ?? '').trim();
  return {
    rawName,
    rawColor,
    hasExplicitName: Boolean(rawName)
  };
};

const normalizeAgentAvatarName = (
  rawName: unknown,
  options: { fallbackWhenEmpty: string; fallbackWhenUnknown: string }
): string => {
  const normalizedLegacy = normalizeLegacyAvatarName(rawName);
  if (!normalizedLegacy) {
    return options.fallbackWhenEmpty;
  }
  if (normalizedLegacy === 'initial') {
    return 'initial';
  }
  const normalized = normalizeAvatarIcon(normalizedLegacy, PROFILE_AVATAR_OPTION_KEYS);
  if (normalized === 'initial') {
    return options.fallbackWhenUnknown;
  }
  return normalized;
};

export const parseAgentAvatarIconConfig = (value: unknown): AgentAvatarIconConfig => {
  const extracted = extractAgentIconFields(value);
  const name = normalizeAgentAvatarName(extracted.rawName, {
    fallbackWhenEmpty: DEFAULT_AGENT_AVATAR_ICON_NAME,
    fallbackWhenUnknown: FALLBACK_AGENT_AVATAR_ICON_NAME
  });
  const color = normalizeAvatarColor(extracted.rawColor || DEFAULT_AVATAR_COLOR);
  return { name, color };
};

export const stringifyAgentAvatarIconConfig = (
  value: Partial<AgentAvatarIconConfig> | null | undefined
): string => {
  const extracted = extractAgentIconFields(value || {});
  const name = normalizeAgentAvatarName(extracted.rawName, {
    fallbackWhenEmpty: DEFAULT_AGENT_AVATAR_ICON_NAME,
    fallbackWhenUnknown: extracted.hasExplicitName
      ? FALLBACK_AGENT_AVATAR_ICON_NAME
      : DEFAULT_AGENT_AVATAR_ICON_NAME
  });
  const color = normalizeAvatarColor(extracted.rawColor || DEFAULT_AVATAR_COLOR);
  return JSON.stringify({ name, color });
};

export const resolveAgentAvatarImageByConfig = (config: Partial<AgentAvatarIconConfig>): string =>
  PROFILE_AVATAR_IMAGE_MAP.get(String(config?.name || '').trim()) || '';

export const resolveAgentAvatarInitial = (value: unknown): string => {
  const text = String(value || '').trim();
  if (!text) return '?';
  return text.slice(0, 1).toUpperCase();
};

export const DEFAULT_AGENT_AVATAR_IMAGE =
  PROFILE_AVATAR_IMAGE_MAP.get(DEFAULT_AGENT_AVATAR_ICON_NAME) || defaultAgentAvatarImage;
