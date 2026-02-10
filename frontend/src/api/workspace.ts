import type { AxiosProgressEvent } from 'axios';

import api from './http';
import type { ApiPayload, QueryParams } from './types';

type RequestConfig = {
  headers?: Record<string, string>;
  responseType?: 'blob' | 'json' | 'text';
  onUploadProgress?: (event: AxiosProgressEvent) => void;
};

const uploadWorkspaceFiles = (formData: FormData, config: RequestConfig = {}) => {
  const headers = { 'Content-Type': 'multipart/form-data', ...(config.headers || {}) };
  return api.post('/workspace/upload', formData, { ...config, headers });
};

export const listWorkspaceEntries = (params: QueryParams) => api.get('/workspace', { params });
export const fetchWorkspaceContent = (params: QueryParams) => api.get('/workspace/content', { params });
export const searchWorkspace = (params: QueryParams) => api.get('/workspace/search', { params });
export const uploadWorkspace = (formData: FormData, config: RequestConfig = {}) =>
  uploadWorkspaceFiles(formData, config);
export const createWorkspaceDir = (payload: ApiPayload) => api.post('/workspace/dir', payload);
export const moveWorkspaceEntry = (payload: ApiPayload) => api.post('/workspace/move', payload);
export const copyWorkspaceEntry = (payload: ApiPayload) => api.post('/workspace/copy', payload);
export const batchWorkspaceAction = (payload: ApiPayload) => api.post('/workspace/batch', payload);
export const saveWorkspaceFile = (payload: ApiPayload) => api.post('/workspace/file', payload);
export const deleteWorkspaceEntry = (params: QueryParams) => api.delete('/workspace', { params });
export const downloadWorkspaceFile = (params: QueryParams) =>
  api.get('/workspace/download', { params, responseType: 'blob' });
export const downloadWorkspaceArchive = (params: QueryParams) =>
  api.get('/workspace/archive', { params, responseType: 'blob' });

export const listWunderEntries = (params: QueryParams) => listWorkspaceEntries(params);
export const listWunderWorkspace = (params: QueryParams) => listWorkspaceEntries(params);
export const fetchWunderWorkspaceContent = (params: QueryParams) => fetchWorkspaceContent(params);
export const searchWunderWorkspace = (params: QueryParams) => searchWorkspace(params);
export const uploadWunderWorkspace = (formData: FormData, config: RequestConfig = {}) =>
  uploadWorkspace(formData, config);
export const createWunderWorkspaceDir = (payload: ApiPayload) => createWorkspaceDir(payload);
export const moveWunderWorkspaceEntry = (payload: ApiPayload) => moveWorkspaceEntry(payload);
export const copyWunderWorkspaceEntry = (payload: ApiPayload) => copyWorkspaceEntry(payload);
export const batchWunderWorkspaceAction = (payload: ApiPayload) => batchWorkspaceAction(payload);
export const saveWunderWorkspaceFile = (payload: ApiPayload) => saveWorkspaceFile(payload);
export const deleteWunderWorkspaceEntry = (params: QueryParams) => deleteWorkspaceEntry(params);
export const downloadWunderWorkspaceFile = (params: QueryParams) => downloadWorkspaceFile(params);
export const downloadWunderWorkspaceArchive = (params: QueryParams) => downloadWorkspaceArchive(params);
