const WORKSPACE_PUBLIC_PREFIX = '/workspaces/';
const WORKSPACE_AGENT_MARKER = '__agent__';
const WORKSPACE_SHORT_AGENT_MARKER = '__a__';
const WORKSPACE_CONTAINER_MARKER = '__c__';
const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'svg']);
const ABSOLUTE_URI_SCHEME_RE = /^[a-zA-Z][a-zA-Z\d+.-]*:/;
const FILE_URI_SCHEME_RE = /^file:/i;
const WINDOWS_DRIVE_RE = /^[a-zA-Z]:[\\/]/;

const getBaseOrigin = () => {
  if (typeof window !== 'undefined' && window.location?.origin) {
    return window.location.origin;
  }
  return 'http://localhost';
};

const decodePath = (value) => {
  try {
    return decodeURIComponent(value);
  } catch (error) {
    return value;
  }
};

const splitPathWithSuffix = (value) => {
  const queryIndex = value.indexOf('?');
  const hashIndex = value.indexOf('#');
  if (queryIndex < 0 && hashIndex < 0) {
    return { path: value, suffix: '' };
  }
  let splitIndex = value.length;
  if (queryIndex >= 0) {
    splitIndex = Math.min(splitIndex, queryIndex);
  }
  if (hashIndex >= 0) {
    splitIndex = Math.min(splitIndex, hashIndex);
  }
  return {
    path: value.slice(0, splitIndex),
    suffix: value.slice(splitIndex)
  };
};

const stripTrailingSlash = (value) => {
  let output = String(value || '').trim();
  while (output.length > 1 && output.endsWith('/')) {
    output = output.slice(0, -1);
  }
  return output;
};

const normalizeLocalAbsolutePath = (raw) => {
  const text = String(raw || '').trim();
  if (!text) return '';
  if (text.startsWith('//') && !FILE_URI_SCHEME_RE.test(text)) {
    return '';
  }
  if (
    ABSOLUTE_URI_SCHEME_RE.test(text) &&
    !FILE_URI_SCHEME_RE.test(text) &&
    !WINDOWS_DRIVE_RE.test(text)
  ) {
    return '';
  }
  let path = text;
  if (FILE_URI_SCHEME_RE.test(path)) {
    try {
      path = new URL(path).pathname || '';
    } catch (error) {
      path = path.replace(/^file:\/*/i, '/');
    }
  }
  const split = splitPathWithSuffix(path);
  let normalized = decodePath(split.path).replace(/\\/g, '/').trim();
  if (!normalized) return '';
  if (/^\/[a-zA-Z]:\//.test(normalized)) {
    normalized = normalized.slice(1);
  }
  return stripTrailingSlash(normalized);
};

const startsWithPath = (value, prefix) => {
  if (!value || !prefix) return false;
  return value === prefix || value.startsWith(`${prefix}/`);
};

export const resolveWorkspaceRelativePathFromLocal = (
  rawPath,
  workspaceId,
  workspaceRoot = ''
) => {
  const workspaceToken = String(workspaceId || '').trim();
  if (!workspaceToken) return '';
  const normalizedPath = normalizeLocalAbsolutePath(rawPath);
  if (!normalizedPath) return '';
  const normalizedRoot = normalizeLocalAbsolutePath(workspaceRoot);
  const rootWithId = normalizedRoot
    ? stripTrailingSlash(`${normalizedRoot}/${workspaceToken}`)
    : '';

  if (rootWithId && startsWithPath(normalizedPath, rootWithId)) {
    return normalizedPath
      .slice(rootWithId.length)
      .replace(/^\/+/, '');
  }

  if (normalizedRoot && startsWithPath(normalizedPath, normalizedRoot)) {
    let relative = normalizedPath.slice(normalizedRoot.length).replace(/^\/+/, '');
    if (relative.startsWith(`${workspaceToken}/`)) {
      relative = relative.slice(workspaceToken.length + 1);
    }
    return relative;
  }

  const token = `/${workspaceToken}/`;
  const tokenIndex = normalizedPath.indexOf(token);
  if (tokenIndex >= 0) {
    return normalizedPath.slice(tokenIndex + token.length);
  }
  if (normalizedPath.endsWith(`/${workspaceToken}`)) {
    return '';
  }
  return '';
};

export const normalizeWorkspaceBareRelativePath = (raw) => {
  const text = String(raw || '').trim();
  if (!text) return '';
  if (
    text.startsWith('/') ||
    text.startsWith('./') ||
    text.startsWith('../') ||
    text.startsWith('?') ||
    text.startsWith('#') ||
    text.startsWith('//')
  ) {
    return '';
  }
  if (WINDOWS_DRIVE_RE.test(text)) {
    return '';
  }
  if (FILE_URI_SCHEME_RE.test(text)) {
    return '';
  }
  if (ABSOLUTE_URI_SCHEME_RE.test(text)) {
    return '';
  }
  const normalized = text.replace(/\\/g, '/').trim();
  const { path } = splitPathWithSuffix(normalized);
  if (!path) return '';
  if (!path.includes('/') && !path.includes('.')) {
    return '';
  }
  const segments = [];
  path.split('/').forEach((segment) => {
    const token = segment.trim();
    if (!token || token === '.') return;
    if (token === '..') {
      if (segments.length) {
        segments.pop();
      }
      return;
    }
    segments.push(token);
  });
  if (!segments.length) return '';
  return segments.join('/');
};

export const normalizeWorkspaceRelativeMarkdownPath = (raw) => {
  const text = String(raw || '').trim();
  if (!text) return '';
  if (
    text.startsWith('/') ||
    text.startsWith('//') ||
    text.startsWith('?') ||
    text.startsWith('#') ||
    ABSOLUTE_URI_SCHEME_RE.test(text)
  ) {
    return '';
  }
  const normalized = text.replace(/\\/g, '/');
  const { path } = splitPathWithSuffix(normalized);
  if (!path.startsWith('./') && !path.startsWith('../')) {
    return '';
  }
  const segments = [];
  path.split('/').forEach((segment) => {
    const token = segment.trim();
    if (!token || token === '.') return;
    if (token === '..') {
      if (segments.length) {
        segments.pop();
      }
      return;
    }
    segments.push(token);
  });
  if (!segments.length) return '';
  return segments.join('/');
};

export const parseWorkspaceResourceUrl = (raw) => {
  const text = String(raw || '').trim();
  if (!text) return null;
  let url;
  try {
    url = new URL(text, getBaseOrigin());
  } catch (error) {
    return null;
  }
  const pathname = url.pathname || '';
  const index = pathname.indexOf(WORKSPACE_PUBLIC_PREFIX);
  if (index < 0) return null;
  const rest = pathname.slice(index + WORKSPACE_PUBLIC_PREFIX.length);
  const parts = rest.split('/').filter(Boolean);
  if (parts.length < 2) return null;
  const workspaceId = parts.shift();
  if (!workspaceId) return null;
  let ownerId = workspaceId;
  let agentId = '';
  let containerId = null;
  const containerRegex = new RegExp(`^(.*)${WORKSPACE_CONTAINER_MARKER}(\\d+)$`);
  const containerMatch = workspaceId.match(containerRegex);
  if (containerMatch) {
    ownerId = containerMatch[1] || workspaceId;
    containerId = Number.parseInt(containerMatch[2], 10);
  } else {
    const fullAgentIndex = workspaceId.indexOf(WORKSPACE_AGENT_MARKER);
    const shortAgentIndex = workspaceId.indexOf(WORKSPACE_SHORT_AGENT_MARKER);
    if (fullAgentIndex >= 0) {
      ownerId = workspaceId.slice(0, fullAgentIndex) || workspaceId;
      agentId = workspaceId.slice(fullAgentIndex + WORKSPACE_AGENT_MARKER.length);
    } else if (shortAgentIndex >= 0) {
      ownerId = workspaceId.slice(0, shortAgentIndex) || workspaceId;
      agentId = workspaceId.slice(shortAgentIndex + WORKSPACE_SHORT_AGENT_MARKER.length);
    }
  }
  const relativeRaw = parts.join('/');
  if (!relativeRaw) return null;
  const relativePath = decodePath(relativeRaw);
  const filename = relativePath.split('/').pop() || decodePath(parts[parts.length - 1] || '');
  const publicPath = `${WORKSPACE_PUBLIC_PREFIX}${workspaceId}/${relativeRaw}`;
  return {
    userId: workspaceId,
    workspaceId,
    ownerId,
    agentId,
    containerId: Number.isFinite(containerId) ? containerId : null,
    relativePath,
    publicPath,
    filename
  };
};

export const isWorkspaceImageUrl = (raw) => {
  const resource = parseWorkspaceResourceUrl(raw);
  if (!resource?.filename) return false;
  return isImagePath(resource.filename);
};

export const isImagePath = (path) => {
  const value = String(path || '').trim();
  if (!value) return false;
  const suffix = value.split('?')[0].split('#')[0].split('.').pop();
  if (!suffix) return false;
  return IMAGE_EXTENSIONS.has(suffix.toLowerCase());
};

export const WORKSPACE_PUBLIC_ROOT = WORKSPACE_PUBLIC_PREFIX;
