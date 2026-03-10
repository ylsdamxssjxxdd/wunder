import { normalizeWorkspacePath } from './workspaceTreeCache';

const WORKSPACE_REFRESH_PATH_KEYS = [
  'path',
  'paths',
  'changed_paths',
  'changedPaths',
  'target_path',
  'targetPath',
  'source_path',
  'sourcePath',
  'destination',
  'destination_path',
  'destinationPath',
  'relative_path',
  'relativePath',
  'file',
  'files',
  'to_path'
];

export const normalizeWorkspaceRefreshPath = (value: unknown): string => {
  const text = String(value || '').trim();
  if (!text || text === '/' || text === '.') return '';
  return normalizeWorkspacePath(text);
};

export const normalizeWorkspaceRefreshContainerId = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) ? parsed : null;
};

export const normalizeWorkspaceRefreshTreeVersion = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
};

export const extractWorkspaceRefreshPaths = (detail: unknown): string[] => {
  if (!detail || typeof detail !== 'object') return [];
  const result = new Set<string>();

  const appendPathLike = (value: unknown) => {
    if (value === null || value === undefined) return;
    if (Array.isArray(value)) {
      value.forEach((item) => appendPathLike(item));
      return;
    }
    if (typeof value === 'string') {
      const normalized = normalizeWorkspaceRefreshPath(value);
      if (normalized || value.trim() === '/' || value.trim() === '.') {
        result.add(normalized);
      }
      return;
    }
    if (typeof value === 'object') {
      const record = value as Record<string, unknown>;
      WORKSPACE_REFRESH_PATH_KEYS.forEach((key) => {
        if (key in record) {
          appendPathLike(record[key]);
        }
      });
      if (record.data && typeof record.data === 'object') {
        appendPathLike(record.data);
      }
      if (record.meta && typeof record.meta === 'object') {
        appendPathLike(record.meta);
      }
    }
  };

  appendPathLike(detail);
  return Array.from(result);
};

export const isWorkspacePathAffected = (
  targetPath: unknown,
  changedPaths: string[]
): boolean => {
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
