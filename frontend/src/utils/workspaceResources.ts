const WORKSPACE_PUBLIC_PREFIX = '/workspaces/';
const WORKSPACE_AGENT_MARKER = '__agent__';
const WORKSPACE_SHORT_AGENT_MARKER = '__a__';
const WORKSPACE_CONTAINER_MARKER = '__c__';
const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'svg']);

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
