export type WorkspaceRefreshEntryLike = {
  path?: string;
  type?: string;
  name?: string;
  childrenLoaded?: boolean;
  children?: WorkspaceRefreshEntryLike[];
};

type WorkspaceRefreshPlanOptions = {
  currentPath?: string;
  changedPaths?: string[];
  entries?: WorkspaceRefreshEntryLike[];
  maxTargets?: number;
};

const normalizeWorkspacePath = (path: unknown): string => {
  if (!path) return '';
  return String(path).replace(/\\/g, '/').replace(/^\/+/, '');
};

const normalizeWorkspaceRefreshPath = (value: unknown): string => {
  const text = String(value || '').trim();
  if (!text || text === '/' || text === '.') return '';
  return normalizeWorkspacePath(text);
};

const isWorkspacePathAffected = (targetPath: unknown, changedPaths: string[]): boolean => {
  if (!Array.isArray(changedPaths) || !changedPaths.length) return true;
  const normalizedTarget = normalizeWorkspaceRefreshPath(targetPath);
  return changedPaths.some((item) => {
    const normalizedChanged = normalizeWorkspaceRefreshPath(item);
    if (!normalizedChanged) return true;
    if (!normalizedTarget) return false;
    return (
      normalizedTarget === normalizedChanged ||
      normalizedTarget.startsWith(`${normalizedChanged}/`) ||
      normalizedChanged.startsWith(`${normalizedTarget}/`)
    );
  });
};

export const findWorkspaceEntryByPath = (
  entries: WorkspaceRefreshEntryLike[] = [],
  targetPath: string
): WorkspaceRefreshEntryLike | null => {
  if (!Array.isArray(entries) || !targetPath) return null;
  for (const entry of entries) {
    if (!entry || typeof entry !== 'object') continue;
    if (entry.path === targetPath) return entry;
    if (Array.isArray(entry.children) && entry.children.length) {
      const nested = findWorkspaceEntryByPath(entry.children, targetPath);
      if (nested) return nested;
    }
  }
  return null;
};

export const getWorkspaceParentPath = (path: string): string => {
  const normalized = normalizeWorkspacePath(path);
  if (!normalized) return '';
  const parts = normalized.split('/').filter(Boolean);
  parts.pop();
  return parts.join('/');
};

export const shouldAcceptWorkspaceTreeVersion = (
  nextTreeVersion: number | null,
  latestAppliedTreeVersion: number
): boolean => {
  if (nextTreeVersion === null) return true;
  return nextTreeVersion > latestAppliedTreeVersion;
};

// Keep refresh targets minimal so frequent workspace updates do not degrade into full reloads.
export const collectWorkspaceRefreshTargets = ({
  currentPath = '',
  changedPaths = [],
  entries = [],
  maxTargets = 6
}: WorkspaceRefreshPlanOptions): { targets: string[]; forceFullReload: boolean } => {
  const normalizedCurrentPath = normalizeWorkspacePath(currentPath);
  const targets = new Set<string>();

  changedPaths.forEach((item) => {
    const normalized = normalizeWorkspaceRefreshPath(item);
    if (!normalized) {
      targets.add(normalizedCurrentPath);
      return;
    }
    if (
      normalizedCurrentPath &&
      (normalizedCurrentPath === normalized ||
        normalizedCurrentPath.startsWith(`${normalized}/`))
    ) {
      targets.add(normalizedCurrentPath);
      return;
    }
    if (
      normalizedCurrentPath &&
      normalized !== normalizedCurrentPath &&
      !normalized.startsWith(`${normalizedCurrentPath}/`)
    ) {
      return;
    }
    const entry = findWorkspaceEntryByPath(entries, normalized);
    if (entry?.type === 'dir') {
      targets.add(normalized);
      return;
    }
    const parentPath = getWorkspaceParentPath(normalized);
    if (normalizedCurrentPath) {
      targets.add(parentPath || normalizedCurrentPath);
    } else {
      targets.add(parentPath);
    }
  });

  const deduped = Array.from(
    new Set(Array.from(targets).map((value) => normalizeWorkspacePath(value)))
  );
  if (deduped.length > maxTargets) {
    return { targets: [], forceFullReload: true };
  }
  return { targets: deduped, forceFullReload: false };
};

export const shouldWorkspacePreviewReload = (
  previewPath: string,
  changedPaths: string[]
): boolean => {
  const normalizedPreviewPath = normalizeWorkspaceRefreshPath(previewPath);
  if (!normalizedPreviewPath) return false;
  if (!Array.isArray(changedPaths) || changedPaths.length === 0) return true;
  return isWorkspacePathAffected(normalizedPreviewPath, changedPaths);
};
