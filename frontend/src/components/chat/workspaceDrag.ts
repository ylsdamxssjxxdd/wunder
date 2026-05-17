import { normalizeWorkspacePath } from '@/utils/workspaceTreeCache';

export const WORKSPACE_DRAG_KEY = 'application/x-wunder-workspace-entry';
const WORKSPACE_DRAG_TEXT_PREFIX = '__wunder_workspace_drag__::';
let currentWorkspaceDragPaths: string[] = [];

const normalizeWorkspaceDragPaths = (paths: unknown): string[] => {
  const source = Array.isArray(paths) ? paths : [paths];
  const result = new Set<string>();
  source.forEach((path) => {
    const normalized = normalizeWorkspacePath(path);
    if (normalized) {
      result.add(normalized);
    }
  });
  return Array.from(result);
};

export const hasWorkspaceDragPaths = (dataTransfer: DataTransfer | null | undefined): boolean =>
  currentWorkspaceDragPaths.length > 0 ||
  Array.from(dataTransfer?.types || []).includes(WORKSPACE_DRAG_KEY);

export const setWorkspaceDragPaths = (
  dataTransfer: DataTransfer | null | undefined,
  paths: unknown
): string[] => {
  const normalized = normalizeWorkspaceDragPaths(paths);
  if (!dataTransfer || !normalized.length) return normalized;
  currentWorkspaceDragPaths = normalized;
  dataTransfer.setData(WORKSPACE_DRAG_KEY, JSON.stringify(normalized));
  dataTransfer.setData('text/plain', `${WORKSPACE_DRAG_TEXT_PREFIX}${JSON.stringify(normalized)}`);
  dataTransfer.setData('text/uri-list', normalized.join('\n'));
  return normalized;
};

export const clearWorkspaceDragPaths = () => {
  currentWorkspaceDragPaths = [];
};

export const readWorkspaceDragPaths = (
  dataTransfer: DataTransfer | null | undefined
): string[] => {
  const raw = dataTransfer?.getData(WORKSPACE_DRAG_KEY) || '';
  if (!raw) {
    if (currentWorkspaceDragPaths.length) return [...currentWorkspaceDragPaths];
    const fallback = String(dataTransfer?.getData('text/plain') || '').trim();
    if (!fallback.startsWith(WORKSPACE_DRAG_TEXT_PREFIX)) return [];
    const payload = fallback.slice(WORKSPACE_DRAG_TEXT_PREFIX.length);
    try {
      const parsed = JSON.parse(payload);
      if (Array.isArray(parsed)) {
        return normalizeWorkspaceDragPaths(parsed);
      }
    } catch (error) {
      return normalizeWorkspaceDragPaths(payload.split(/\r?\n/));
    }
    return [];
  }
  try {
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      return normalizeWorkspaceDragPaths(parsed);
    }
  } catch (error) {
    // Fall back to plain-text payloads from other drag sources.
  }
  const plain = normalizeWorkspaceDragPaths(raw.split(/\r?\n/));
  if (plain.length) return plain;
  if (currentWorkspaceDragPaths.length) return [...currentWorkspaceDragPaths];
  const fallback = String(dataTransfer?.getData('text/plain') || '').trim();
  if (!fallback.startsWith(WORKSPACE_DRAG_TEXT_PREFIX)) return [];
  const payload = fallback.slice(WORKSPACE_DRAG_TEXT_PREFIX.length);
  try {
    const parsed = JSON.parse(payload);
    if (Array.isArray(parsed)) {
      return normalizeWorkspaceDragPaths(parsed);
    }
  } catch (error) {
    return normalizeWorkspaceDragPaths(payload.split(/\r?\n/));
  }
  return [];
};
