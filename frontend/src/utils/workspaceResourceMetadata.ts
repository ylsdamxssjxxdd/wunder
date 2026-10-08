import workspaceIconsTheme from '@/assets/vscode-icons-theme.json';
const DRAWIO_EXTENSIONS = new Set(['dio', 'drawio', 'drawio.xml']);

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
