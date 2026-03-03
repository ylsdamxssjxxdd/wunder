export const WORLD_VOICE_PREVIEW_TEXT = '[Voice]';

export type WorldVoicePayload = {
  kind?: string;
  path: string;
  duration_ms?: number;
  mime_type?: string;
  name?: string;
  size?: number;
  container_id?: number;
  owner_user_id?: string;
};

const DEFAULT_WORLD_VOICE_CONTAINER_ID = 0;

const normalizePath = (value: unknown): string =>
  String(value || '')
    .replace(/\\/g, '/')
    .replace(/^\/+/, '')
    .trim();

const normalizeNumber = (value: unknown): number => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return 0;
  return Math.round(parsed);
};

export const isWorldVoiceContentType = (value: unknown): boolean => {
  const normalized = String(value || '')
    .trim()
    .toLowerCase();
  if (!normalized) return false;
  return (
    normalized === 'voice' ||
    normalized === 'audio' ||
    normalized.startsWith('audio/') ||
    normalized.includes('voice')
  );
};

export const parseWorldVoicePayload = (content: unknown): WorldVoicePayload | null => {
  const raw = String(content || '').trim();
  if (!raw) return null;
  let payload: Record<string, unknown> | null = null;
  try {
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      payload = parsed as Record<string, unknown>;
    }
  } catch {
    payload = null;
  }
  const path = normalizePath(payload?.path ?? raw);
  if (!path) return null;
  const containerId = normalizeNumber(payload?.container_id);
  const durationMs = normalizeNumber(payload?.duration_ms);
  const size = normalizeNumber(payload?.size);
  return {
    kind: String(payload?.kind || 'voice').trim() || 'voice',
    path,
    duration_ms: durationMs > 0 ? durationMs : undefined,
    mime_type: String(payload?.mime_type || 'audio/wav').trim() || 'audio/wav',
    name: String(payload?.name || '').trim() || undefined,
    size: size > 0 ? size : undefined,
    container_id: containerId > 0 ? containerId : DEFAULT_WORLD_VOICE_CONTAINER_ID,
    owner_user_id: String(payload?.owner_user_id || '').trim() || undefined
  };
};

export const buildWorldVoicePayloadContent = (payload: {
  path: string;
  durationMs: number;
  mimeType?: string;
  name?: string;
  size?: number;
  containerId?: number;
  ownerUserId?: string;
}): string => {
  const path = normalizePath(payload.path);
  if (!path) {
    throw new Error('voice path is required');
  }
  const durationMs = normalizeNumber(payload.durationMs);
  const size = normalizeNumber(payload.size);
  const normalized = {
    kind: 'voice',
    path,
    duration_ms: durationMs,
    mime_type: String(payload.mimeType || 'audio/wav').trim() || 'audio/wav',
    name: String(payload.name || '').trim() || undefined,
    size: size > 0 ? size : undefined,
    container_id: normalizeNumber(payload.containerId) || DEFAULT_WORLD_VOICE_CONTAINER_ID,
    owner_user_id: String(payload.ownerUserId || '').trim() || undefined
  };
  return JSON.stringify(normalized);
};

export const formatWorldVoiceDuration = (durationMs: unknown): string => {
  const totalMs = normalizeNumber(durationMs);
  if (!totalMs) return '0:00';
  const totalSeconds = Math.max(1, Math.round(totalMs / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
};
