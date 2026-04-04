export type ComposerDraftAttachment = {
  id: string;
  type: string;
  name: string;
  content: string;
  mime_type?: string;
  converter?: string;
  public_path?: string;
  source_public_path?: string;
  derived_attachments?: ComposerDraftAttachment[];
  requested_frame_rate?: number;
  applied_frame_rate?: number;
  duration_ms?: number;
  frame_count?: number;
  has_audio?: boolean;
  warnings?: string[];
};

export type ComposerDraftState = {
  content: string;
  attachments: ComposerDraftAttachment[];
};

const composerDraftCache = new Map<string, ComposerDraftState>();

const normalizeKey = (value: unknown): string => String(value || '').trim();

const cloneAttachment = (attachment: ComposerDraftAttachment): ComposerDraftAttachment => ({
  id: String(attachment.id || ''),
  type: String(attachment.type || ''),
  name: String(attachment.name || ''),
  content: String(attachment.content || ''),
  ...(attachment.mime_type ? { mime_type: String(attachment.mime_type) } : {}),
  ...(attachment.converter ? { converter: String(attachment.converter) } : {}),
  ...(attachment.public_path ? { public_path: String(attachment.public_path) } : {}),
  ...(attachment.source_public_path
    ? { source_public_path: String(attachment.source_public_path) }
    : {}),
  ...(Array.isArray(attachment.derived_attachments)
    ? { derived_attachments: attachment.derived_attachments.map(cloneAttachment) }
    : {}),
  ...(Number.isFinite(attachment.requested_frame_rate)
    ? { requested_frame_rate: Number(attachment.requested_frame_rate) }
    : {}),
  ...(Number.isFinite(attachment.applied_frame_rate)
    ? { applied_frame_rate: Number(attachment.applied_frame_rate) }
    : {}),
  ...(Number.isFinite(attachment.duration_ms) ? { duration_ms: Number(attachment.duration_ms) } : {}),
  ...(Number.isFinite(attachment.frame_count) ? { frame_count: Number(attachment.frame_count) } : {}),
  ...(attachment.has_audio === true ? { has_audio: true } : {}),
  ...(Array.isArray(attachment.warnings)
    ? {
        warnings: attachment.warnings
          .map((item) => String(item || '').trim())
          .filter((item) => item)
      }
    : {})
});

const cloneState = (state: ComposerDraftState): ComposerDraftState => ({
  content: String(state.content || ''),
  attachments: Array.isArray(state.attachments) ? state.attachments.map(cloneAttachment) : []
});

export const readComposerDraftState = (key: unknown): ComposerDraftState | null => {
  const normalized = normalizeKey(key);
  if (!normalized) return null;
  const state = composerDraftCache.get(normalized);
  return state ? cloneState(state) : null;
};

export const writeComposerDraftState = (key: unknown, state: ComposerDraftState) => {
  const normalized = normalizeKey(key);
  if (!normalized) return;
  composerDraftCache.set(normalized, cloneState(state));
};

export const clearComposerDraftState = (key: unknown) => {
  const normalized = normalizeKey(key);
  if (!normalized) return;
  composerDraftCache.delete(normalized);
};
