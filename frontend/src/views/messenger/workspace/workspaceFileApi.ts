/**
 * Thin request layer for the sidebar cloud-directory area (B2).
 *
 * Contract notes:
 * - Every `/wunder/workspace*` endpoint returns a **bare object** (no `{ data }` wrapper),
 *   so callers read `response.data` directly.
 * - No call here passes a container parameter: the cloud workspace is a single
 *   per-user root and the parameter is converged away on the server.
 */

import type { AxiosProgressEvent } from 'axios';

import {
  batchWorkspaceAction,
  clearWorkspace,
  copyWorkspaceEntry,
  createWorkspaceDir,
  deleteWorkspaceEntry,
  downloadWorkspaceArchive,
  downloadWorkspaceFile,
  fetchWorkspaceContent,
  fetchWorkspaceStats,
  listWorkspaceEntries,
  moveWorkspaceEntry,
  saveWorkspaceFile,
  uploadWorkspace
} from '@/api/workspace';
import type { QueryParams } from '@/api/types';
import { resolveApiError } from '@/utils/apiError';
import { getFilenameFromHeaders, saveObjectUrlAsFile } from '@/utils/workspaceResourceCards';
import {
  joinWorkspacePath,
  normalizeWorkspaceEntries,
  normalizeWorkspaceRelativePath,
  sortWorkspaceEntries,
  type WorkspaceEntry
} from './workspaceFileModel';

export type WorkspaceDirectoryPage = {
  path: string;
  entries: WorkspaceEntry[];
  total: number;
  offset: number;
  limit: number;
};

export type WorkspaceStatsSnapshot = {
  path: string;
  files: number;
  dirs: number;
  usedBytes: number;
  truncated: boolean;
  quotaBytes: number | null;
};

export type WorkspaceActionOutcome = {
  ok: boolean;
  message: string;
  files: string[];
};

export const resolveWorkspaceErrorMessage = (error: unknown, fallback = ''): string =>
  resolveApiError(error, fallback).message || fallback;

export const isWorkspaceEndpointMissing = (error: unknown): boolean => {
  const status = resolveApiError(error, '').status;
  return status === 404 || status === 501;
};

export const isWorkspaceRequestCancelled = (error: unknown): boolean =>
  String((error as { code?: string } | null)?.code || '') === 'ERR_CANCELED';

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

export const fetchWorkspaceDirectory = async (
  path: string,
  options: { offset?: number; limit?: number; signal?: AbortSignal } = {}
): Promise<WorkspaceDirectoryPage> => {
  const params: QueryParams = {
    path: normalizeWorkspaceRelativePath(path),
    offset: Math.max(0, Number(options.offset) || 0),
    limit: Math.max(0, Number(options.limit) || 0),
    sort_by: 'name',
    order: 'asc'
  };
  const response = await listWorkspaceEntries(params);
  const payload = asRecord(response?.data);
  const entries = sortWorkspaceEntries(normalizeWorkspaceEntries(payload.entries));
  return {
    path: normalizeWorkspaceRelativePath(payload.path ?? params.path),
    entries,
    total: Number(payload.total) || entries.length,
    offset: Number(payload.offset) || 0,
    limit: Number(payload.limit) || Number(params.limit) || 0
  };
};

export const fetchWorkspaceStatsSnapshot = async (
  path = '',
  options: { recentLimit?: number; signal?: AbortSignal } = {}
): Promise<WorkspaceStatsSnapshot> => {
  const params: QueryParams = {
    path: normalizeWorkspaceRelativePath(path),
    recent_limit: Math.max(0, Number(options.recentLimit) || 8)
  };
  const response = await fetchWorkspaceStats(params, { signal: options.signal });
  const payload = asRecord(response?.data);
  const quotaRaw = payload.quota_bytes;
  const quota = Number(quotaRaw);
  return {
    path: normalizeWorkspaceRelativePath(payload.path ?? params.path),
    files: Number(payload.files) || 0,
    dirs: Number(payload.dirs) || 0,
    usedBytes: Number(payload.used_bytes) || 0,
    truncated: payload.truncated === true,
    quotaBytes: quotaRaw === null || quotaRaw === undefined || !Number.isFinite(quota) ? null : quota
  };
};

export const readWorkspaceFileContent = async (
  path: string,
  maxBytes: number
): Promise<{ content: string; size: number; truncated: boolean; updatedTime: string }> => {
  const response = await fetchWorkspaceContent({
    path: normalizeWorkspaceRelativePath(path),
    include_content: true,
    max_bytes: Math.max(1024, Number(maxBytes) || 0)
  });
  const payload = asRecord(response?.data);
  return {
    content: typeof payload.content === 'string' ? payload.content : '',
    size: Number(payload.size) || 0,
    truncated: payload.truncated === true,
    updatedTime: String(payload.updated_time || '')
  };
};

export const writeWorkspaceFileContent = async (path: string, content: string): Promise<void> => {
  await saveWorkspaceFile({
    path: normalizeWorkspaceRelativePath(path),
    content,
    create_if_missing: false
  });
};

export const createWorkspaceDirectory = async (parentPath: string, name: string): Promise<void> => {
  await createWorkspaceDir({ path: joinWorkspacePath(parentPath, name) });
};

export const moveWorkspacePath = async (source: string, destination: string): Promise<void> => {
  await moveWorkspaceEntry({
    source: normalizeWorkspaceRelativePath(source),
    destination: normalizeWorkspaceRelativePath(destination)
  });
};

export const copyWorkspacePath = async (source: string, destination: string): Promise<void> => {
  await copyWorkspaceEntry({
    source: normalizeWorkspaceRelativePath(source),
    destination: normalizeWorkspaceRelativePath(destination)
  });
};

export const deleteWorkspacePath = async (path: string): Promise<void> => {
  await deleteWorkspaceEntry({ path: normalizeWorkspaceRelativePath(path) });
};

export type WorkspaceBatchOutcome = {
  ok: boolean;
  message: string;
  succeeded: number;
  failed: number;
};

export const deleteWorkspacePaths = async (paths: string[]): Promise<WorkspaceBatchOutcome> => {
  const response = await batchWorkspaceAction({
    action: 'delete',
    paths: paths.map((path) => normalizeWorkspaceRelativePath(path)).filter(Boolean)
  });
  const payload = asRecord(response?.data);
  const failed = Array.isArray(payload.failed) ? payload.failed.length : 0;
  const succeeded = Array.isArray(payload.succeeded) ? payload.succeeded.length : 0;
  return {
    ok: payload.ok !== false && failed === 0,
    message: String(payload.message || ''),
    succeeded,
    failed
  };
};

export const clearWorkspaceRoot = async (): Promise<void> => {
  await clearWorkspace({});
};

const resolveDownloadFilename = (headers: unknown, fallback: string): string =>
  getFilenameFromHeaders((headers || {}) as Record<string, unknown>, fallback) || fallback;

const triggerBlobDownload = (blob: Blob, filename: string): void => {
  const objectUrl = URL.createObjectURL(blob);
  saveObjectUrlAsFile(objectUrl, filename);
  window.setTimeout(() => URL.revokeObjectURL(objectUrl), 1000);
};

export const downloadWorkspacePath = async (path: string, fallbackName?: string): Promise<void> => {
  const normalized = normalizeWorkspaceRelativePath(path);
  const response = await downloadWorkspaceFile({ path: normalized });
  const filename = resolveDownloadFilename(
    response.headers,
    fallbackName || normalized.split('/').pop() || 'download'
  );
  triggerBlobDownload(response.data as Blob, filename);
};

export const downloadWorkspaceDirectoryArchive = async (
  path: string,
  fallbackName?: string
): Promise<void> => {
  const normalized = normalizeWorkspaceRelativePath(path);
  const response = await downloadWorkspaceArchive({ path: normalized });
  const filename = resolveDownloadFilename(
    response.headers,
    fallbackName || `${normalized ? normalized.split('/').pop() : 'workspace'}.zip`
  );
  triggerBlobDownload(response.data as Blob, filename);
};

export const fetchWorkspacePathBlob = async (
  path: string,
  options: { preview?: string; signal?: AbortSignal } = {}
): Promise<{ blob: Blob; filename: string }> => {
  const normalized = normalizeWorkspaceRelativePath(path);
  const params: QueryParams = { path: normalized };
  if (options.preview) params.preview = options.preview;
  const response = await downloadWorkspaceFile(params, {
    responseType: 'blob',
    signal: options.signal
  });
  return {
    blob: response.data as Blob,
    filename: resolveDownloadFilename(response.headers, normalized.split('/').pop() || 'download')
  };
};

export const uploadWorkspaceEntry = (
  file: File,
  targetPath: string,
  options: {
    relativePath?: string;
    signal?: AbortSignal;
    onProgress?: (loaded: number, total: number) => void;
  } = {}
): Promise<WorkspaceActionOutcome> => {
  const formData = new FormData();
  formData.append('path', normalizeWorkspaceRelativePath(targetPath));
  formData.append('files', file, file.name);
  const relativePath = String(options.relativePath || '').trim();
  if (relativePath) {
    formData.append('relative_paths', relativePath);
  }
  return uploadWorkspace(formData, {
    signal: options.signal,
    onUploadProgress: (event: AxiosProgressEvent) => {
      if (!options.onProgress) return;
      const total = Number(event.total) || Number(file.size) || 0;
      const loaded = Number(event.loaded) || 0;
      options.onProgress(loaded, total);
    }
  }).then((response) => {
    const payload = asRecord(response?.data);
    const files = Array.isArray(payload.files) ? payload.files.map((item) => String(item || '')) : [];
    return {
      ok: payload.ok !== false,
      message: String(payload.message || ''),
      files
    };
  });
};
