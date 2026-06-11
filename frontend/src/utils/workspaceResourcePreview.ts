import { isDesktopLocalModeEnabled } from '@/config/desktop';
import { t } from '@/i18n';
import workspaceIconsTheme from '@/assets/vscode-icons-theme.json';

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

type WorkspaceIconTheme = {
  file?: string;
  fileExtensions?: Record<string, unknown>;
  fileNames?: Record<string, unknown>;
  iconDefinitions?: Record<string, { iconPath?: string } | unknown>;
};

const resolveImportMetaBaseUrl = (): string => {
  const meta = import.meta as ImportMeta & { env?: { BASE_URL?: string } };
  return String(meta.env?.BASE_URL || '/').replace(/\/+$/, '/');
};

const WORKSPACE_ICON_BASE = `${resolveImportMetaBaseUrl()}vscode-icons`;
const WORKSPACE_ICON_PATH_RE = /^(\.\.\/|\.\/)+/;

const ICON_ID_FALLBACK_BY_EXTENSION = new Map<string, string>([
  ['dio', '_f_drawio'],
  ['drawio', '_f_drawio'],
  ['drawio.xml', '_f_drawio'],
  ['doc', '_f_word'],
  ['docx', '_f_word'],
  ['pdf', '_f_pdf'],
  ['png', '_f_image'],
  ['jpg', '_f_image'],
  ['jpeg', '_f_image'],
  ['gif', '_f_image'],
  ['bmp', '_f_image'],
  ['webp', '_f_image'],
  ['svg', '_f_svg'],
  ['txt', '_f_text'],
  ['md', '_f_markdown'],
  ['log', '_f_log'],
  ['csv', '_f_text'],
  ['tsv', '_f_text'],
  ['mp3', '_f_audio'],
  ['wav', '_f_audio'],
  ['flac', '_f_audio'],
  ['aac', '_f_audio'],
  ['ogg', '_f_audio'],
  ['m4a', '_f_audio'],
  ['mp4', '_f_video'],
  ['mov', '_f_video'],
  ['avi', '_f_video'],
  ['mkv', '_f_video'],
  ['webm', '_f_video'],
  ['ppt', '_f_powerpoint'],
  ['pptx', '_f_powerpoint'],
  ['xls', '_f_excel'],
  ['xlsx', '_f_excel']
]);

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

export const decodeWorkspaceResourceLabel = (value = ''): string => {
  const text = String(value || '').trim();
  if (!text) return '';
  if (!/%[0-9a-fA-F]{2}/.test(text)) return text;
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
};

export const extractWorkspaceResourceExtension = (value = ''): string => {
  const raw = String(value || '').trim();
  if (!raw) return '';
  const base = raw.split('?')[0].split('#')[0];
  const name = base.split('/').pop() || '';
  const lowered = name.toLowerCase();
  if (lowered.endsWith('.drawio.xml')) {
    return 'drawio.xml';
  }
  const dotIndex = name.lastIndexOf('.');
  if (dotIndex <= 0 || dotIndex >= name.length - 1) return '';
  return name.slice(dotIndex + 1).toLowerCase();
};

export const normalizeWorkspacePreviewFilename = (label = '', filename = ''): string => {
  const decodedLabel = decodeWorkspaceResourceLabel(label);
  const decodedFilename = decodeWorkspaceResourceLabel(filename);
  return decodedLabel || decodedFilename || 'resource';
};

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

const DOC_ICON_BASE = `${resolveImportMetaBaseUrl()}doc-icons`;
const fallbackDocIcon = `${DOC_ICON_BASE}/other.png`;
const drawioDocIcon = `${DOC_ICON_BASE}/processon_flow.png`;

const theme = (workspaceIconsTheme || {}) as WorkspaceIconTheme;
const iconDefinitions = (theme.iconDefinitions || {}) as Record<string, { iconPath?: string }>;
const fileExtensions = new Map(
  Object.entries(theme.fileExtensions || {}).map(([key, value]) => [String(key).trim().toLowerCase(), String(value || '')])
);
const fileNames = new Map(
  Object.entries(theme.fileNames || {}).map(([key, value]) => [String(key).trim().toLowerCase(), String(value || '')])
);
const defaultFileIconId = String(theme.file || '').trim();

const normalizeThemeIconPath = (iconPath: string | undefined): string => {
  const rawPath = String(iconPath || '').trim();
  if (!rawPath) {
    return '';
  }
  const normalizedPath = rawPath.replace(WORKSPACE_ICON_PATH_RE, '');
  return `${WORKSPACE_ICON_BASE}/${normalizedPath}`;
};

const resolveThemeIconPathById = (iconId = ''): string => {
  if (!iconId) {
    return '';
  }
  return normalizeThemeIconPath(iconDefinitions[iconId]?.iconPath);
};

export const resolveWorkspaceFileCardIconPath = (filename = ''): string => {
  const normalizedName = String(filename || '').trim().toLowerCase();
  const extension = extractWorkspaceResourceExtension(filename);
  if (DRAWIO_EXTENSIONS.has(extension)) {
    return drawioDocIcon;
  }
  const directId =
    (normalizedName && fileNames.get(normalizedName)) ||
    (extension && fileExtensions.get(extension)) ||
    ICON_ID_FALLBACK_BY_EXTENSION.get(extension) ||
    defaultFileIconId;
  const resolved = resolveThemeIconPathById(String(directId || ''));
  if (resolved) {
    return resolved;
  }
  const fallbackResolved = resolveThemeIconPathById(defaultFileIconId);
  return fallbackResolved || fallbackDocIcon;
};
