import api from './http';

import type { ApiId, ApiPayload, QueryParams } from './types';
import { resolveAccessToken } from '@/api/requestAuth';
import { resolveApiBase } from '@/config/runtime';

type OpenSocketOptions = {
  protocols?: string[] | string;
  allowQueryToken?: boolean;
  params?: QueryParams;
};

type OpenStreamOptions = {
  allowQueryToken?: boolean;
  params?: QueryParams;
};

const resolveWsBase = (): string => {
  const base = resolveApiBase() || api.defaults.baseURL || '';
  const trimmed = base.replace(/\/$/, '');
  if (!trimmed) return '';
  if (/^https?:\/\//i.test(trimmed)) {
    return trimmed.replace(/^http/i, 'ws');
  }
  if (trimmed.startsWith('/')) {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    return `${protocol}//${window.location.host}${trimmed}`;
  }
  return trimmed;
};

const resolveHttpBase = (): string => {
  const base = resolveApiBase() || api.defaults.baseURL || '';
  const trimmed = base.replace(/\/$/, '');
  if (!trimmed) return '';
  if (/^https?:\/\//i.test(trimmed)) {
    return trimmed;
  }
  if (trimmed.startsWith('/')) {
    return `${window.location.origin}${trimmed}`;
  }
  return trimmed;
};

const buildWsProtocols = (token: string | null, options: OpenSocketOptions = {}): string[] => {
  const protocols: string[] = [];
  const append = (value: unknown) => {
    const cleaned = String(value || '').trim();
    if (!cleaned || /\s/.test(cleaned)) return;
    if (!protocols.includes(cleaned)) {
      protocols.push(cleaned);
    }
  };
  if (Array.isArray(options.protocols)) {
    options.protocols.forEach(append);
  } else if (typeof options.protocols === 'string') {
    append(options.protocols);
  }
  append('wunder');
  if (token && !options.allowQueryToken) {
    append(`wunder-auth.${token}`);
  }
  return protocols;
};

const buildWsUrl = (path: string, params: URLSearchParams): string => {
  const base = resolveWsBase();
  const suffix = params.toString();
  return suffix ? `${base}${path}?${suffix}` : `${base}${path}`;
};

const buildHttpUrl = (path: string, params: URLSearchParams): string => {
  const base = resolveHttpBase();
  const suffix = params.toString();
  return suffix ? `${base}${path}?${suffix}` : `${base}${path}`;
};

export const listBeeroomGroups = (params: QueryParams = {}) =>
  api.get('/beeroom/groups', { params, timeout: 60000 });

export const createBeeroomGroup = (payload: ApiPayload) =>
  api.post('/beeroom/groups', payload, { timeout: 60000 });

export const getBeeroomGroup = (groupId: ApiId, params: QueryParams = {}) =>
  api.get(`/beeroom/groups/${encodeURIComponent(groupId)}`, { params, timeout: 60000 });

export const updateBeeroomGroup = (groupId: ApiId, payload: ApiPayload) =>
  api.put(`/beeroom/groups/${encodeURIComponent(groupId)}`, payload, { timeout: 60000 });

export const deleteBeeroomGroup = (
  groupId: ApiId,
  params: QueryParams = {}
) =>
  api.delete(`/beeroom/groups/${encodeURIComponent(groupId)}`, { params, timeout: 60000 });

export const moveBeeroomAgents = (groupId: ApiId, payload: ApiPayload) =>
  api.post(`/beeroom/groups/${encodeURIComponent(groupId)}/move_agents`, payload, {
    timeout: 60000
  });

export const listBeeroomMissions = (groupId: ApiId, params: QueryParams = {}) =>
  api.get(`/beeroom/groups/${encodeURIComponent(groupId)}/missions`, { params, timeout: 60000 });

export const getBeeroomMission = (groupId: ApiId, missionId: ApiId) =>
  api.get(`/beeroom/groups/${encodeURIComponent(groupId)}/missions/${encodeURIComponent(missionId)}`, {
    timeout: 60000
  });

export const fetchBeeroomOrchestrationPrompts = () =>
  api.get('/beeroom/orchestration/prompts', { timeout: 60000 });

export const updateBeeroomOrchestrationSessionContext = (payload: ApiPayload) =>
  api.post('/beeroom/orchestration/session-context', payload, { timeout: 60000 });

export const getBeeroomOrchestrationState = (params: QueryParams = {}) =>
  api.get('/beeroom/orchestration/state', { params, timeout: 60000 });

export const createBeeroomOrchestrationState = (payload: ApiPayload) =>
  api.post('/beeroom/orchestration/state/create', payload, { timeout: 60000 });

export const exitBeeroomOrchestrationState = (payload: ApiPayload) =>
  api.post('/beeroom/orchestration/state/exit', payload, { timeout: 60000 });

export const listBeeroomOrchestrationHistory = (params: QueryParams = {}) =>
  api.get('/beeroom/orchestration/history', { params, timeout: 60000 });

export const deleteBeeroomOrchestrationHistory = (payload: ApiPayload) =>
  api.delete('/beeroom/orchestration/history', { data: payload, timeout: 60000 });

export const restoreBeeroomOrchestrationHistory = (payload: ApiPayload) =>
  api.post('/beeroom/orchestration/history/restore', payload, { timeout: 60000 });

export const branchBeeroomOrchestrationHistory = (payload: ApiPayload) =>
  api.post('/beeroom/orchestration/history/branch', payload, { timeout: 60000 });

export const truncateBeeroomOrchestrationHistory = (payload: ApiPayload) =>
  api.post('/beeroom/orchestration/history/truncate', payload, { timeout: 60000 });

export const reserveBeeroomOrchestrationRound = (payload: ApiPayload) =>
  api.post('/beeroom/orchestration/rounds/reserve', payload, { timeout: 60000 });

export const finalizeBeeroomOrchestrationRound = (payload: ApiPayload) =>
  api.post('/beeroom/orchestration/rounds/finalize', payload, { timeout: 60000 });

export const cancelBeeroomOrchestrationRound = (payload: ApiPayload) =>
  api.post('/beeroom/orchestration/rounds/cancel', payload, { timeout: 60000 });

export type StartBeeroomDemoRunRequest = {
  seed?: number;
  worker_count_mode?: 'random' | 'all' | string;
  worker_count?: number;
  speed?: 'fast' | 'normal' | 'slow' | string;
  scenario?: string;
  tool_profile?: 'safe' | string;
  mother_agent_id?: string;
};

export const startBeeroomDemoRun = (
  groupId: ApiId,
  payload: StartBeeroomDemoRunRequest = {}
) =>
  api.post(`/beeroom/groups/${encodeURIComponent(groupId)}/demo_runs`, payload, {
    timeout: 60000
  });

export const getBeeroomDemoRun = (groupId: ApiId, runId: ApiId) =>
  api.get(
    `/beeroom/groups/${encodeURIComponent(groupId)}/demo_runs/${encodeURIComponent(String(runId || '').trim())}`,
    {
      timeout: 60000
    }
  );

export const cancelBeeroomDemoRun = (groupId: ApiId, runId: ApiId) =>
  api.post(
    `/beeroom/groups/${encodeURIComponent(groupId)}/demo_runs/${encodeURIComponent(String(runId || '').trim())}/cancel`,
    {},
    { timeout: 60000 }
  );

export const openBeeroomSocket = (options: OpenSocketOptions = {}): WebSocket => {
  const token = resolveAccessToken();
  const params = new URLSearchParams();
  if (options.allowQueryToken && token) {
    params.set('access_token', token);
  }
  if (options.params) {
    Object.entries(options.params).forEach(([key, value]) => {
      if (value === undefined || value === null || value === '') return;
      params.set(key, String(value));
    });
  }
  const protocols = buildWsProtocols(token, options);
  const url = buildWsUrl('/beeroom/ws', params);
  return protocols.length ? new WebSocket(url, protocols) : new WebSocket(url);
};

export const openBeeroomChatStream = (groupId: ApiId, options: OpenStreamOptions = {}): EventSource => {
  const normalizedGroupId = encodeURIComponent(String(groupId || '').trim());
  const token = resolveAccessToken();
  const params = new URLSearchParams();
  if (options.allowQueryToken && token) {
    params.set('access_token', token);
  }
  if (options.params) {
    Object.entries(options.params).forEach(([key, value]) => {
      if (value === undefined || value === null || value === '') return;
      params.set(key, String(value));
    });
  }
  const url = buildHttpUrl(`/beeroom/groups/${normalizedGroupId}/chat/stream`, params);
  return new EventSource(url);
};

export type HivePackImportRequest = {
  file: Blob | File;
  options?: ApiPayload;
  groupId?: ApiId;
};

export const importBeeroomHivePack = (payload: HivePackImportRequest) => {
  const form = new FormData();
  form.append('file', payload.file);
  if (payload.options && Object.keys(payload.options).length) {
    form.append('options', JSON.stringify(payload.options));
  }
  const groupId = String(payload.groupId ?? '').trim();
  if (groupId) {
    form.append('group_id', groupId);
  }
  return api.post('/beeroom/packs/import', form, {
    headers: { 'Content-Type': 'multipart/form-data' },
    timeout: 180000
  });
};

export const getBeeroomHivePackImportJob = (jobId: ApiId) =>
  api.get(`/beeroom/packs/import/${encodeURIComponent(String(jobId || ''))}`, {
    timeout: 60000
  });

export const exportBeeroomHivePack = (payload: ApiPayload) =>
  api.post('/beeroom/packs/export', payload, {
    timeout: 180000
  });

export const getBeeroomHivePackExportJob = (jobId: ApiId) =>
  api.get(`/beeroom/packs/export/${encodeURIComponent(String(jobId || ''))}`, {
    timeout: 60000
  });

export const downloadBeeroomHivePack = (jobId: ApiId) =>
  api.get(`/beeroom/packs/export/${encodeURIComponent(String(jobId || ''))}/download`, {
    responseType: 'blob',
    timeout: 180000
  });
