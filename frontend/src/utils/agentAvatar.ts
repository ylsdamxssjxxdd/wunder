import {
  AGENT_AVATAR_IMAGE_MAP,
  AGENT_AVATAR_OPTION_KEYS,
  DEFAULT_AGENT_AVATAR_IMAGE_KEY
} from '@/utils/agentAvatarCatalog';
import { DEFAULT_AVATAR_COLOR, normalizeAvatarColor, normalizeAvatarIcon } from '@/utils/userPreferences';

export type AgentAvatarIconConfig = {
  kind: 'static' | 'companion';
  name: string;
  color: string;
  scope?: 'global' | 'private';
  id?: string;
  show?: boolean;
  messageHints?: boolean;
  scale?: number;
};

const DEFAULT_AGENT_AVATAR_ICON_NAME = DEFAULT_AGENT_AVATAR_IMAGE_KEY;
const FALLBACK_AGENT_AVATAR_ICON_NAME = 'initial';

const normalizeAgentAvatarSequenceKey = (rawValue: string): string => {
  const sequence = Number.parseInt(rawValue, 10);
  if (!Number.isFinite(sequence) || sequence < 0) {
    return DEFAULT_AGENT_AVATAR_ICON_NAME;
  }
  const candidate = `avatar-${String(sequence).padStart(3, '0')}`;
  return AGENT_AVATAR_IMAGE_MAP.has(candidate) ? candidate : DEFAULT_AGENT_AVATAR_ICON_NAME;
};

const normalizeLegacyAvatarName = (value: unknown): string => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (!text) return '';
  const agentAvatarMatch = text.match(/^agent-avatar-(\d{1,4})$/);
  if (agentAvatarMatch) {
    return normalizeAgentAvatarSequenceKey(agentAvatarMatch[1]);
  }
  const nextAvatarMatch = text.match(/^avatar-(\d{1,4})$/);
  if (nextAvatarMatch) {
    return normalizeAgentAvatarSequenceKey(nextAvatarMatch[1]);
  }
  const qqAvatarMatch = text.match(/^qq-avatar-(\d{1,4})$/);
  if (qqAvatarMatch) {
    return normalizeAgentAvatarSequenceKey(qqAvatarMatch[1]);
  }
  if (text === 'default') {
    return DEFAULT_AGENT_AVATAR_ICON_NAME;
  }
  return text;
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};

const normalizeIconKind = (value: unknown): 'static' | 'companion' => {
  const text = String(value || '').trim().toLowerCase();
  return text === 'companion' ? 'companion' : 'static';
};

const normalizeCompanionScope = (value: unknown): 'global' | 'private' => {
  const text = String(value || '').trim().toLowerCase();
  return text === 'private' ? 'private' : 'global';
};

const normalizeBoolean = (value: unknown, fallback = false): boolean => {
  if (value === true) return true;
  if (value === false) return false;
  return fallback;
};

const normalizeScale = (value: unknown, fallback = 1): number => {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return fallback;
  }
  return Math.min(1.8, Math.max(0.5, numeric));
};

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
  rawKind: 'static' | 'companion';
  rawScope: 'global' | 'private';
  rawId: string;
  rawShow: boolean;
  rawMessageHints: boolean;
  rawScale: number;
  hasExplicitName: boolean;
};

const extractAgentIconFields = (value: unknown): ExtractedIconFields => {
  if (typeof value === 'string') {
    const text = value.trim();
    if (!text) {
      return {
        rawName: '',
        rawColor: '',
        rawKind: 'static',
        rawScope: 'global',
        rawId: '',
        rawShow: true,
        rawMessageHints: true,
        rawScale: 1,
        hasExplicitName: false
      };
    }
    const parsed = tryParseJsonRecord(text);
    if (!parsed) {
      return {
        rawName: text,
        rawColor: '',
        rawKind: 'static',
        rawScope: 'global',
        rawId: '',
        rawShow: true,
        rawMessageHints: true,
        rawScale: 1,
        hasExplicitName: true
      };
    }
    const rawName = String(
      parsed.name ?? parsed.icon ?? parsed.avatar_icon ?? parsed.avatarIcon ?? parsed.displayName ?? ''
    ).trim();
    const rawColor = String(parsed.color ?? parsed.avatar_color ?? parsed.avatarColor ?? '').trim();
    const rawKind = normalizeIconKind(parsed.kind ?? parsed.type);
    const rawScope = normalizeCompanionScope(parsed.scope);
    const rawId = String(parsed.id ?? parsed.companion_id ?? parsed.companionId ?? '').trim();
    const rawShow = normalizeBoolean(parsed.show, true);
    const rawMessageHints = normalizeBoolean(parsed.messageHints ?? parsed.message_hints, true);
    const rawScale = normalizeScale(parsed.scale, 1);
    return {
      rawName,
      rawColor,
      rawKind,
      rawScope,
      rawId,
      rawShow,
      rawMessageHints,
      rawScale,
      hasExplicitName: Boolean(rawName || rawId)
    };
  }
  const record = asRecord(value);
  const rawName = String(
    record.name ?? record.icon ?? record.avatar_icon ?? record.avatarIcon ?? record.displayName ?? ''
  ).trim();
  const rawColor = String(record.color ?? record.avatar_color ?? record.avatarColor ?? '').trim();
  const rawKind = normalizeIconKind(record.kind ?? record.type);
  const rawScope = normalizeCompanionScope(record.scope);
  const rawId = String(record.id ?? record.companion_id ?? record.companionId ?? '').trim();
  const rawShow = normalizeBoolean(record.show, true);
  const rawMessageHints = normalizeBoolean(record.messageHints ?? record.message_hints, true);
  const rawScale = normalizeScale(record.scale, 1);
  return {
    rawName,
    rawColor,
    rawKind,
    rawScope,
    rawId,
    rawShow,
    rawMessageHints,
    rawScale,
    hasExplicitName: Boolean(rawName || rawId)
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
  const normalized = normalizeAvatarIcon(normalizedLegacy, AGENT_AVATAR_OPTION_KEYS);
  if (normalized === 'initial') {
    return options.fallbackWhenUnknown;
  }
  return normalized;
};

export const parseAgentAvatarIconConfig = (value: unknown): AgentAvatarIconConfig => {
  const extracted = extractAgentIconFields(value);
  const isCompanion = extracted.rawKind === 'companion' || Boolean(extracted.rawId);
  const name = normalizeAgentAvatarName(extracted.rawName || extracted.rawId, {
    fallbackWhenEmpty: DEFAULT_AGENT_AVATAR_ICON_NAME,
    fallbackWhenUnknown: FALLBACK_AGENT_AVATAR_ICON_NAME
  });
  const color = normalizeAvatarColor(extracted.rawColor || DEFAULT_AVATAR_COLOR);
  if (isCompanion) {
    return {
      kind: 'companion',
      name,
      color,
      scope: extracted.rawScope,
      id: extracted.rawId || name,
      show: extracted.rawShow,
      messageHints: extracted.rawMessageHints,
      scale: extracted.rawScale
    };
  }
  return { kind: 'static', name, color };
};

export const resolveAgentAvatarConfiguredColor = (value: unknown, fallback = ''): string => {
  const extracted = extractAgentIconFields(value);
  if (extracted.rawColor) {
    return normalizeAvatarColor(extracted.rawColor);
  }
  const normalizedFallback = String(fallback || '').trim();
  return normalizedFallback ? normalizeAvatarColor(normalizedFallback) : '';
};

export const stringifyAgentAvatarIconConfig = (
  value: Partial<AgentAvatarIconConfig> | null | undefined
): string => {
  const extracted = extractAgentIconFields(value || {});
  const kind = normalizeIconKind((value as Record<string, unknown> | null | undefined)?.kind);
  const name = normalizeAgentAvatarName(extracted.rawName || extracted.rawId, {
    fallbackWhenEmpty: DEFAULT_AGENT_AVATAR_ICON_NAME,
    fallbackWhenUnknown: extracted.hasExplicitName
      ? FALLBACK_AGENT_AVATAR_ICON_NAME
      : DEFAULT_AGENT_AVATAR_ICON_NAME
  });
  const color = normalizeAvatarColor(extracted.rawColor || DEFAULT_AVATAR_COLOR);
  if (kind === 'companion' || extracted.rawId) {
    return JSON.stringify({
      kind: 'companion',
      scope: extracted.rawScope,
      id: extracted.rawId || name,
      color,
      show: extracted.rawShow,
      messageHints: extracted.rawMessageHints,
      scale: extracted.rawScale
    });
  }
  return JSON.stringify({ kind: 'static', name, color });
};

export const resolveAgentAvatarImageByConfig = (config: Partial<AgentAvatarIconConfig>): string =>
  config?.kind === 'companion' ? '' : AGENT_AVATAR_IMAGE_MAP.get(String(config?.name || '').trim()) || '';

export const isAgentAvatarCompanionConfig = (config: Partial<AgentAvatarIconConfig>): boolean =>
  String(config?.kind || '').trim().toLowerCase() === 'companion' || Boolean(String(config?.id || '').trim());

export const resolveAgentAvatarCompanionId = (config: Partial<AgentAvatarIconConfig>): string =>
  String(config?.id || '').trim();

export const resolveAgentAvatarInitial = (value: unknown): string => {
  const text = String(value || '').trim();
  if (!text) return '?';
  return text.slice(0, 1).toUpperCase();
};

export const DEFAULT_AGENT_AVATAR_IMAGE =
  AGENT_AVATAR_IMAGE_MAP.get(DEFAULT_AGENT_AVATAR_ICON_NAME)
  || AGENT_AVATAR_IMAGE_MAP.get(DEFAULT_AGENT_AVATAR_IMAGE_KEY)
  || '';
