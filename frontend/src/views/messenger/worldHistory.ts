const WORLD_HISTORY_MEDIA_EXTENSIONS = new Set([
  'png',
  'jpg',
  'jpeg',
  'gif',
  'webp',
  'bmp',
  'svg',
  'ico',
  'mp4',
  'mov',
  'avi',
  'mkv',
  'webm',
  'm4v'
]);

const WORLD_HISTORY_DOCUMENT_EXTENSIONS = new Set([
  'pdf',
  'doc',
  'docx',
  'xls',
  'xlsx',
  'ppt',
  'pptx',
  'txt',
  'md',
  'csv',
  'rtf'
]);

export type WorldHistoryRecordCategory = 'media' | 'document' | 'other_file' | 'text';

export const normalizeWorldHistoryText = (value: unknown): string =>
  String(value || '')
    .replace(/!\[[^\]]*\]\(([^)]+)\)/g, '$1')
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '$1 $2')
    .replace(/`{1,3}([^`]+)`{1,3}/g, '$1')
    .replace(/[>#*_~]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();

export const extractPathExtension = (value: unknown): string => {
  const cleaned = String(value || '')
    .trim()
    .replace(/^@/, '')
    .replace(/^['"]|['"]$/g, '')
    .split(/[?#]/)[0];
  if (!cleaned) return '';
  const lastSegment = cleaned.split('/').filter(Boolean).pop() || '';
  const index = lastSegment.lastIndexOf('.');
  if (index <= 0 || index >= lastSegment.length - 1) return '';
  return lastSegment.slice(index + 1).toLowerCase();
};

export const extractWorldHistoryTokenExtensions = (content: string): string[] => {
  const output = new Set<string>();
  const append = (source: string) => {
    const ext = extractPathExtension(source);
    if (ext) output.add(ext);
  };
  content.replace(/!\[[^\]]*]\(([^)]+)\)/g, (_match, path) => {
    append(path);
    return '';
  });
  content.replace(/\[[^\]]+]\(([^)]+)\)/g, (_match, path) => {
    append(path);
    return '';
  });
  content.replace(/https?:\/\/[^\s)]+/gi, (url) => {
    append(url);
    return '';
  });
  content.replace(/@(?:"([^"]+)"|'([^']+)'|([^\s]+))/g, (_match, quoted, singleQuoted, plain) => {
    append(quoted || singleQuoted || plain || '');
    return '';
  });
  return Array.from(output);
};

export const classifyWorldHistoryMessage = (message: Record<string, unknown>): WorldHistoryRecordCategory => {
  const contentType = String(message?.content_type || '')
    .trim()
    .toLowerCase();
  if (contentType.includes('image') || contentType.includes('video')) {
    return 'media';
  }
  if (contentType.includes('document')) {
    return 'document';
  }
  if (contentType.includes('file')) {
    return 'other_file';
  }
  const content = String(message?.content || '');
  const extensions = extractWorldHistoryTokenExtensions(content);
  if (extensions.some((ext) => WORLD_HISTORY_MEDIA_EXTENSIONS.has(ext))) {
    return 'media';
  }
  if (extensions.some((ext) => WORLD_HISTORY_DOCUMENT_EXTENSIONS.has(ext))) {
    return 'document';
  }
  if (extensions.length > 0 || /@(?:"[^"]+"|'[^']+'|[^\s]+)/.test(content)) {
    return 'other_file';
  }
  if (/<img\b|!\[[^\]]*]\([^)]+\)/i.test(content)) {
    return 'media';
  }
  return 'text';
};

export const resolveWorldHistoryIcon = (category: WorldHistoryRecordCategory): string => {
  if (category === 'media') return 'fa-image';
  if (category === 'document') return 'fa-file-lines';
  if (category === 'other_file') return 'fa-file';
  return 'fa-comment-dots';
};
