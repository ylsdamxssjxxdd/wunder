import { extractWorkspaceResourceExtension, resolveWorkspaceFileCardIconPath } from '@/utils/workspaceResourceMetadata';
import type { WorkspaceDroppedFile } from '@/utils/workspaceDrop';

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg', 'tif', 'tiff', 'avif']);
const GENERIC_CLIPBOARD_IMAGE_NAME = /^(?:image|screenshot|screen[ _-]?shot|clipboard)(?:[ _-]?\d+)?\.[a-z0-9]+$/i;

export const isComposerImageFile = (file: File): boolean => {
  const mimeType = String(file?.type || '').trim().toLowerCase();
  if (mimeType.startsWith('image/')) return true;
  return IMAGE_EXTENSIONS.has(extractWorkspaceResourceExtension(file?.name || ''));
};

export const createComposerImagePreview = (file: File): string =>
  isComposerImageFile(file) ? URL.createObjectURL(file) : '';

export const resolveComposerAttachmentIcon = (name: string): string =>
  resolveWorkspaceFileCardIconPath(name);

export const revokeComposerImagePreview = (url: string | undefined): void => {
  if (String(url || '').startsWith('blob:')) {
    URL.revokeObjectURL(String(url));
  }
};

const clipboardImageExtension = (file: File): string => {
  const fromName = extractWorkspaceResourceExtension(file.name);
  if (fromName) return fromName;
  const subtype = String(file.type || '').split('/')[1]?.split(';')[0].trim().toLowerCase();
  return subtype === 'jpeg' ? 'jpg' : subtype || 'png';
};

export const normalizeClipboardDroppedFile = (item: WorkspaceDroppedFile): WorkspaceDroppedFile => {
  const file = item.file;
  const originalName = String(file.name || '').trim();
  if (!isComposerImageFile(file) || !GENERIC_CLIPBOARD_IMAGE_NAME.test(originalName)) {
    return item;
  }
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  const suffix = Math.random().toString(36).slice(2, 8);
  const name = `pasted-${stamp}-${suffix}.${clipboardImageExtension(file)}`;
  return {
    file: new File([file], name, { type: file.type, lastModified: file.lastModified }),
    relativePath: name
  };
};
