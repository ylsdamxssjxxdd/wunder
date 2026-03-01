export type ComposerDraftAttachment = {
  id: string;
  type: string;
  name: string;
  content: string;
  mime_type?: string;
  converter?: string;
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
  ...(attachment.converter ? { converter: String(attachment.converter) } : {})
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
