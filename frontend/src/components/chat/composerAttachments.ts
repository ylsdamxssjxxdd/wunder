import { extractWorkspaceResourceExtension, resolveWorkspaceFileCardIconPath } from '@/utils/workspaceResourceMetadata';
import type { WorkspaceDroppedFile } from '@/utils/workspaceDrop';

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg', 'tif', 'tiff', 'avif']);
/**
 * 剪贴板里的通用名（image.png / 截图 2026-11-24.png / Clipboard(2).png…）本身没有信息量，
 * 落盘时换成一个短名；带真实内容的名字原样保留。
 */
const GENERIC_CLIPBOARD_IMAGE_NAME =
  /^(?:image|img|photo|picture|screenshot|screen[ _-]?shot|clipboard|untitled|paste|pasted|图片|截图|未命名)(?:[ _-]?\(?\d+\)?)?\.[a-z0-9]+$/i;

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

/**
 * 粘贴进来的通用名图片改成短名（`pasted-image-1.png`）：
 * 每次粘贴都落到独立批次目录，序号只需在本次粘贴内唯一即可。
 */
export const normalizeClipboardDroppedFile = (
  item: WorkspaceDroppedFile,
  index = 0
): WorkspaceDroppedFile => {
  const file = item.file;
  const originalName = String(file.name || '').trim();
  if (!isComposerImageFile(file) || !GENERIC_CLIPBOARD_IMAGE_NAME.test(originalName)) {
    return item;
  }
  const name = `pasted-image-${index + 1}.${clipboardImageExtension(file)}`;
  return {
    file: new File([file], name, { type: file.type, lastModified: file.lastModified }),
    relativePath: name
  };
};

/** 粗略显示宽度：CJK / 全角 / emoji 记 2 个单位，其余记 1，用于按视觉宽度而不按字符数截断。 */
const nameDisplayUnits = (value: string): number =>
  Array.from(value).reduce(
    (total, char) => total + ((char.codePointAt(0) || 0) > 0x2e7f ? 2 : 1),
    0
  );

/**
 * 附件名只做展示裁剪：保留扩展名，超宽的主体折成 `前若干字…`。
 * 完整名字仍保留在 `title` 里，发送给后端的也是原名。
 */
export const compactComposerAttachmentName = (name: string, maxUnits = 22): string => {
  const value = String(name || '').trim();
  if (!value || nameDisplayUnits(value) <= maxUnits) return value;
  const dot = value.lastIndexOf('.');
  const extension = dot > 0 ? value.slice(dot) : '';
  const bodyChars = Array.from(extension ? value.slice(0, dot) : value);
  // 预留 1 个单位给省略号，扩展名整体保留。
  const room = Math.max(6, maxUnits - nameDisplayUnits(extension) - 1);
  let used = 0;
  let kept = '';
  for (const char of bodyChars) {
    const units = (char.codePointAt(0) || 0) > 0x2e7f ? 2 : 1;
    if (used + units > room) break;
    used += units;
    kept += char;
  }
  return `${kept}…${extension}`;
};
