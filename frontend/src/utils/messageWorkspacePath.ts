import {
  normalizeWorkspaceBareRelativePath,
  normalizeWorkspaceRelativeMarkdownPath,
  resolveWorkspaceRelativePathFromLocal
} from './workspaceResources';

export const normalizeWorkspaceOwnerId = (value: unknown): string =>
  String(value || '')
    .trim()
    .replace(/[^a-zA-Z0-9_-]/g, '_');

const normalizeUploadPath = (value: unknown): string =>
  String(value || '')
    .replace(/\\/g, '/')
    .replace(/^\/+/, '')
    .trim();

const encodeWorkspacePath = (value: string): string =>
  String(value || '')
    .split('/')
    .map((part) => encodeURIComponent(part))
    .join('/');

export const buildWorkspacePublicPath = (
  ownerId: string,
  relativePath: string,
  containerId?: number | null
): string => {
  const safeOwner = normalizeWorkspaceOwnerId(ownerId);
  const normalized = normalizeUploadPath(relativePath);
  if (!safeOwner || !normalized) return '';
  if (containerId !== null && Number.isFinite(Number(containerId))) {
    return `/workspaces/${safeOwner}__c__${Number(containerId)}/${encodeWorkspacePath(normalized)}`;
  }
  return `/workspaces/${safeOwner}/${encodeWorkspacePath(normalized)}`;
};

export const buildWorkspacePublicPathFromScope = (
  workspaceScopeId: string,
  relativePath: string
): string => {
  const scope = String(workspaceScopeId || '').trim();
  const normalized = normalizeUploadPath(relativePath);
  if (!scope || !normalized) return '';
  return `/workspaces/${scope}/${encodeWorkspacePath(normalized)}`;
};

export const buildWorkspaceScopeId = (ownerId: string, containerId?: number | null): string => {
  const safeOwner = normalizeWorkspaceOwnerId(ownerId);
  if (!safeOwner) return '';
  if (containerId !== null && Number.isFinite(Number(containerId))) {
    return `${safeOwner}__c__${Number(containerId)}`;
  }
  return safeOwner;
};

export const buildAgentWorkspaceScopeId = (ownerId: string, agentId?: string | null): string => {
  const safeOwner = normalizeWorkspaceOwnerId(ownerId);
  const safeAgent = normalizeWorkspaceOwnerId(agentId);
  if (!safeOwner) return '';
  if (!safeAgent) return safeOwner;
  return `${safeOwner}__a__${safeAgent}`;
};

type ResolveMarkdownWorkspacePathOptions = {
  rawPath: string;
  ownerId?: string;
  workspaceScopeId?: string;
  containerId?: number | null;
  desktopLocalMode?: boolean;
  workspaceRoot?: string;
};

export const resolveMarkdownWorkspacePath = ({
  rawPath,
  ownerId,
  workspaceScopeId,
  containerId,
  desktopLocalMode = false,
  workspaceRoot = ''
}: ResolveMarkdownWorkspacePathOptions): string => {
  const safeOwner = normalizeWorkspaceOwnerId(ownerId);
  const scopeId = String(workspaceScopeId || '').trim() || buildWorkspaceScopeId(safeOwner, containerId);
  if (!scopeId) return '';

  const buildPublicPath = (relativePath: string) => {
    if (scopeId.includes('__a__')) {
      return buildWorkspacePublicPathFromScope(scopeId, relativePath);
    }
    return buildWorkspacePublicPath(safeOwner, relativePath, containerId);
  };

  // Prefer explicit Markdown paths, then bare relative workspace paths,
  // and finally local absolute paths that can be mapped back into workspace URLs.
  const markdownRelative = normalizeWorkspaceRelativeMarkdownPath(rawPath);
  if (markdownRelative) {
    return buildPublicPath(markdownRelative);
  }

  const bareRelative = normalizeWorkspaceBareRelativePath(rawPath);
  if (bareRelative) {
    return buildPublicPath(bareRelative);
  }

  if (!desktopLocalMode) return '';
  const localRelative = resolveWorkspaceRelativePathFromLocal(rawPath, scopeId, workspaceRoot);
  if (!localRelative) return '';
  return buildPublicPath(localRelative);
};
