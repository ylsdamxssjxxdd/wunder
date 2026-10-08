/**
 * Pure helpers for the sidebar cloud-directory area (B2).
 *
 * Everything here is side-effect free and allocation-light on purpose: the
 * tree keeps a bounded number of entries per directory, and a directory
 * refresh patches that single node instead of rebuilding a tree structure.
 */

export type WorkspaceEntryKind = 'file' | 'dir';

export type WorkspaceEntry = {
  name: string;
  path: string;
  kind: WorkspaceEntryKind;
  size: number;
  updatedTime: string;
  loadedAt: number;
};

export type WorkspaceVisibleRow = {
  key: string;
  path: string;
  name: string;
  /** `more` rows are synthetic pagination footers owned by a directory. */
  kind: WorkspaceEntryKind | 'more';
  size: number;
  updatedTime: string;
  depth: number;
  expanded: boolean;
  loading: boolean;
  expandable: boolean;
  remaining?: number;
};

/** Entries requested per directory page. Keeps a single render under one screen. */
export const WORKSPACE_DIRECTORY_PAGE_SIZE = 200;
/** Row height used by the scroll viewport math (px). */
export const WORKSPACE_ROW_HEIGHT = 26;
/** Extra rows rendered above/below the viewport. */
export const WORKSPACE_ROW_OVERSCAN = 6;

export const normalizeWorkspaceRelativePath = (value: unknown): string => {
  if (value === null || value === undefined) return '';
  let normalized = String(value).trim();
  if (!normalized) return '';
  normalized = normalized.replace(/^\\\\\?\\/, '');
  normalized = normalized.replace(/^\/\/\?\//, '');
  normalized = normalized.replace(/\\/g, '/');
  normalized = normalized.replace(/^\/+/, '');
  normalized = normalized.replace(/\/+$/, '');
  if (normalized === '.') return '';
  return normalized;
};

export const joinWorkspacePath = (basePath: unknown, name: unknown): string => {
  const base = normalizeWorkspaceRelativePath(basePath);
  const child = String(name || '').replace(/\\/g, '/').replace(/^\/+/, '').replace(/\/+$/, '');
  if (!child) return base;
  if (!base) return child;
  return `${base}/${child}`;
};

export const workspaceParentPath = (path: unknown): string => {
  const normalized = normalizeWorkspaceRelativePath(path);
  if (!normalized) return '';
  const index = normalized.lastIndexOf('/');
  return index < 0 ? '' : normalized.slice(0, index);
};

export const workspaceBaseName = (path: unknown): string => {
  const normalized = normalizeWorkspaceRelativePath(path);
  if (!normalized) return '';
  const index = normalized.lastIndexOf('/');
  return index < 0 ? normalized : normalized.slice(index + 1);
};

export const workspacePathChain = (path: unknown): string[] => {
  const normalized = normalizeWorkspaceRelativePath(path);
  if (!normalized) return [];
  const segments = normalized.split('/').filter(Boolean);
  const chain: string[] = [];
  let current = '';
  segments.forEach((segment) => {
    current = current ? `${current}/${segment}` : segment;
    chain.push(current);
  });
  return chain;
};

export const normalizeWorkspaceEntry = (raw: unknown): WorkspaceEntry | null => {
  if (!raw || typeof raw !== 'object') return null;
  const source = raw as Record<string, unknown>;
  const name = String(source.name || '').trim() || workspaceBaseName(source.path);
  const path = normalizeWorkspaceRelativePath(source.path);
  if (!name || !path) return null;
  const type = String(source.type || '').trim().toLowerCase();
  return {
    name,
    path,
    kind: type === 'dir' || type === 'directory' ? 'dir' : 'file',
    size: Number.isFinite(Number(source.size)) ? Number(source.size) : 0,
    updatedTime: String(source.updated_time || source.updatedTime || ''),
    loadedAt: Date.now()
  };
};

export const normalizeWorkspaceEntries = (raw: unknown): WorkspaceEntry[] => {
  if (!Array.isArray(raw)) return [];
  const result: WorkspaceEntry[] = [];
  raw.forEach((item) => {
    const entry = normalizeWorkspaceEntry(item);
    if (entry) result.push(entry);
  });
  return result;
};

const workspaceNameCollator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/** Directories first, then natural name order — matches the server's default sort. */
export const sortWorkspaceEntries = (entries: WorkspaceEntry[]): WorkspaceEntry[] =>
  entries.slice().sort((left, right) => {
    if (left.kind !== right.kind) return left.kind === 'dir' ? -1 : 1;
    return workspaceNameCollator.compare(left.name, right.name);
  });

/**
 * Reuse unchanged entry objects so row identity (and therefore DOM keying)
 * stays stable across a single-directory refresh.
 */
export const patchWorkspaceEntries = (
  previous: WorkspaceEntry[],
  next: WorkspaceEntry[]
): WorkspaceEntry[] => {
  if (!previous.length) return next;
  const previousByPath = new Map<string, WorkspaceEntry>();
  previous.forEach((entry) => previousByPath.set(entry.path, entry));
  return next.map((entry) => {
    const existing = previousByPath.get(entry.path);
    if (
      existing &&
      existing.name === entry.name &&
      existing.kind === entry.kind &&
      existing.size === entry.size &&
      existing.updatedTime === entry.updatedTime
    ) {
      return existing;
    }
    return entry;
  });
};

export const appendWorkspacePage = (
  current: WorkspaceEntry[],
  page: WorkspaceEntry[]
): WorkspaceEntry[] => {
  if (!page.length) return current;
  const seen = new Set(current.map((entry) => entry.path));
  const merged = current.slice();
  page.forEach((entry) => {
    if (seen.has(entry.path)) return;
    seen.add(entry.path);
    merged.push(entry);
  });
  return merged;
};

export const removeWorkspaceEntries = (
  current: WorkspaceEntry[],
  removedPaths: Iterable<string>
): WorkspaceEntry[] => {
  const removed = new Set(removedPaths);
  if (!removed.size) return current;
  const next = current.filter((entry) => !removed.has(entry.path));
  return next.length === current.length ? current : next;
};

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

export const formatWorkspaceBytes = (value: unknown): string => {
  const bytes = Number(value);
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  let size = bytes;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < BYTE_UNITS.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }
  const digits = unitIndex === 0 ? 0 : size >= 100 ? 0 : size >= 10 ? 1 : 2;
  return `${size.toFixed(digits)} ${BYTE_UNITS[unitIndex]}`;
};

export const formatWorkspaceTimestamp = (value: unknown): string => {
  const raw = String(value || '').trim();
  if (!raw) return '';
  const parsed = new Date(raw.includes('T') ? raw : raw.replace(' ', 'T'));
  if (Number.isNaN(parsed.getTime())) return raw;
  const pad = (input: number) => String(input).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())} ${pad(
    parsed.getHours()
  )}:${pad(parsed.getMinutes())}`;
};

export const workspaceFileExtension = (name: unknown): string => {
  const normalized = String(name || '').trim().toLowerCase();
  if (!normalized) return '';
  const index = normalized.lastIndexOf('.');
  if (index <= 0 || index === normalized.length - 1) return '';
  return normalized.slice(index + 1);
};

export const isSameWorkspacePath = (left: unknown, right: unknown): boolean =>
  normalizeWorkspaceRelativePath(left) === normalizeWorkspaceRelativePath(right);

/** Reject names that would escape the workspace root or break path routing. */
export const isValidWorkspaceEntryName = (value: unknown): boolean => {
  const name = String(value || '').trim();
  if (!name || name === '.' || name === '..') return false;
  if (name.length > 128) return false;
  if (/[\\/:*?"<>|\u0000-\u001f]/.test(name)) return false;
  return true;
};
