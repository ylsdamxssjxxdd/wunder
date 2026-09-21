import { isDesktopLocalModeEnabled } from '@/config/desktop';
import { t } from '@/i18n';
import { extractWorkspaceResourceExtension } from './workspaceResourceMetadata';
export { decodeWorkspaceResourceLabel, extractWorkspaceResourceExtension, normalizeWorkspacePreviewFilename, resolveWorkspaceFileCardIconPath } from './workspaceResourceMetadata';

export const WORKSPACE_RESOURCE_PREVIEW_TEXT_MAX_BYTES = 512 * 1024;

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'svg', 'wmf', 'emf']);
const PDF_EXTENSIONS = new Set(['pdf']);
const AUDIO_EXTENSIONS = new Set(['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a']);
const VIDEO_EXTENSIONS = new Set(['mp4', 'mov', 'avi', 'mkv', 'webm']);
const DRAWIO_EXTENSIONS = new Set(['dio', 'drawio', 'drawio.xml']);
const TEXT_EXTENSIONS = new Set([
  'txt',
  'md',
  'markdown',
  'log',
  'json',
  'yaml',
  'yml',
  'toml',
  'ini',
  'cfg',
  'conf',
  'properties',
  'env',
  'xml',
  'csv',
  'tsv',
  'py',
  'pyi',
  'pyw',
  'js',
  'jsx',
  'ts',
  'tsx',
  'css',
  'scss',
  'sass',
  'less',
  'html',
  'htm',
  'xhtml',
  'sh',
  'bash',
  'zsh',
  'fish',
  'bat',
  'cmd',
  'ps1',
  'sql',
  'c',
  'cc',
  'cpp',
  'cxx',
  'h',
  'hh',
  'hpp',
  'hxx',
  'rs',
  'java',
  'kt',
  'kts',
  'go',
  'php',
  'vue',
  'astro',
  'svelte',
  'dockerfile',
  'gitignore'
]);
const ONLYOFFICE_WORD_EXTENSIONS = new Set([
  'doc',
  'docm',
  'docx',
  'dot',
  'dotm',
  'dotx',
  'epub',
  'fb2',
  'fodt',
  'hml',
  'hwp',
  'hwpx',
  'mht',
  'mhtml',
  'odt',
  'ott',
  'pages',
  'rtf',
  'stw',
  'sxw',
  'wps',
  'wpt'
]);
const ONLYOFFICE_EXCEL_EXTENSIONS = new Set([
  'csv',
  'et',
  'ett',
  'fods',
  'numbers',
  'ods',
  'ots',
  'sxc',
  'tsv',
  'xls',
  'xlsb',
  'xlsm',
  'xlsx',
  'xlt',
  'xltm',
  'xltx'
]);
const ONLYOFFICE_PPT_EXTENSIONS = new Set([
  'dps',
  'dpt',
  'fodp',
  'key',
  'odg',
  'odp',
  'otp',
  'pot',
  'potm',
  'potx',
  'pps',
  'ppsm',
  'ppsx',
  'ppt',
  'pptm',
  'pptx',
  'sxi'
]);
const ONLYOFFICE_PDF_EXTENSIONS = new Set(['djvu', 'oxps', 'pdf', 'xps']);
const ONLYOFFICE_DIAGRAM_EXTENSIONS = new Set(['vsdm', 'vsdx', 'vssm', 'vssx', 'vstm', 'vstx']);
const ONLYOFFICE_TEXT_ALIAS_EXTENSIONS = new Set([
  'astro',
  'bash',
  'bat',
  'c',
  'cc',
  'cfg',
  'cmd',
  'conf',
  'cpp',
  'cs',
  'css',
  'cxx',
  'dart',
  'fish',
  'go',
  'gradle',
  'h',
  'hpp',
  'java',
  'jl',
  'js',
  'json',
  'jsx',
  'kt',
  'kts',
  'less',
  'log',
  'lua',
  'php',
  'pl',
  'pm',
  'ps1',
  'py',
  'r',
  'rb',
  'rs',
  'sass',
  'scss',
  'sh',
  'sql',
  'svelte',
  'swift',
  'toml',
  'ts',
  'tsx',
  'vue',
  'yaml',
  'yml',
  'zsh'
]);
const ONLYOFFICE_EXTENSIONS = new Set([
  ...ONLYOFFICE_WORD_EXTENSIONS,
  ...ONLYOFFICE_EXCEL_EXTENSIONS,
  ...ONLYOFFICE_PPT_EXTENSIONS,
  ...ONLYOFFICE_PDF_EXTENSIONS,
  ...ONLYOFFICE_DIAGRAM_EXTENSIONS,
  ...ONLYOFFICE_TEXT_ALIAS_EXTENSIONS
]);

const IMAGE_MIME_TYPES: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  bmp: 'image/bmp',
  webp: 'image/webp',
  svg: 'image/svg+xml',
  wmf: 'image/png',
  emf: 'image/png'
};

const AUDIO_MIME_TYPES: Record<string, string> = {
  aac: 'audio/aac',
  flac: 'audio/flac',
  m4a: 'audio/mp4',
  mp3: 'audio/mpeg',
  ogg: 'audio/ogg',
  wav: 'audio/wav'
};

const VIDEO_MIME_TYPES: Record<string, string> = {
  avi: 'video/x-msvideo',
  mkv: 'video/x-matroska',
  mov: 'video/quicktime',
  mp4: 'video/mp4',
  webm: 'video/webm'
};

export type WorkspaceResourcePreviewKind =
  | 'image'
  | 'svg'
  | 'pdf'
  | 'audio'
  | 'video'
  | 'text'
  | 'onlyoffice'
  | 'drawio'
  | 'unsupported';

export const resolveWorkspaceResourcePreviewKind = (
  filename = '',
  sizeBytes?: number | null
): WorkspaceResourcePreviewKind => {
  const extension = extractWorkspaceResourceExtension(filename);
  const safeSize = Number(sizeBytes);
  const isTooLarge = Number.isFinite(safeSize) && safeSize > WORKSPACE_RESOURCE_PREVIEW_TEXT_MAX_BYTES;
  if (DRAWIO_EXTENSIONS.has(extension)) return 'drawio';
  if (TEXT_EXTENSIONS.has(extension) && !isTooLarge) return 'text';
  if (ONLYOFFICE_EXTENSIONS.has(extension)) return 'onlyoffice';
  if (extension === 'svg') return 'svg';
  if (IMAGE_EXTENSIONS.has(extension)) return 'image';
  if (PDF_EXTENSIONS.has(extension)) return 'pdf';
  if (AUDIO_EXTENSIONS.has(extension)) return 'audio';
  if (VIDEO_EXTENSIONS.has(extension)) return 'video';
  return 'unsupported';
};

export const resolveWorkspacePreviewUnsupportedHint = (): string =>
  isDesktopLocalModeEnabled()
    ? t('workspace.preview.unsupportedHintLocal')
    : t('workspace.preview.unsupportedHint');

export const resolveWorkspacePreviewTooLargeHint = (): string =>
  isDesktopLocalModeEnabled()
    ? t('workspace.preview.tooLargeHintLocal')
    : t('workspace.preview.tooLargeHint');

export const resolveWorkspaceResourceMimeType = (
  kind: WorkspaceResourcePreviewKind,
  extension = ''
): string => {
  if (kind === 'svg') return IMAGE_MIME_TYPES.svg;
  if (kind === 'image') return IMAGE_MIME_TYPES[extension] || '';
  if (kind === 'audio') return AUDIO_MIME_TYPES[extension] || '';
  if (kind === 'video') return VIDEO_MIME_TYPES[extension] || '';
  if (kind === 'pdf') return 'application/pdf';
  return '';
};

export const normalizeWorkspacePreviewBlob = (
  blob: Blob,
  kind: WorkspaceResourcePreviewKind,
  extension = ''
): Blob => {
  if (!(blob instanceof Blob)) return blob;
  const expectedMime = resolveWorkspaceResourceMimeType(kind, extension);
  if (!expectedMime || blob.type === expectedMime) {
    return blob;
  }
  if (!blob.type || blob.type === 'application/octet-stream' || kind === 'svg') {
    return blob.slice(0, blob.size, expectedMime);
  }
  return blob;
};
