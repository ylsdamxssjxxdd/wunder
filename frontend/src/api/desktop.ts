import axios from 'axios';
import type { AxiosResponse } from 'axios';

import { getDesktopLocalToken } from '@/config/desktop';

import type { ApiPayload } from './types';

export type DesktopContainerRoot = {
  container_id: number;
  root: string;
};

export type DesktopContainerMount = {
  container_id: number;
  root: string;
  cloud_workspace_id: string;
  seed_status: string;
};

export type DesktopLanMeshSettings = {
  enabled: boolean;
  peer_id: string;
  display_name: string;
  listen_host: string;
  listen_port: number;
  discovery_port: number;
  discovery_interval_ms: number;
  peer_ttl_ms: number;
  allow_subnets: string[];
  deny_subnets: string[];
  peer_blacklist: string[];
  shared_secret: string;
  max_inbound_dedup: number;
  relay_http_fallback: boolean;
  peer_ws_path: string;
  peer_http_path: string;
};

export type DesktopLanPeer = {
  peer_id: string;
  user_id: string;
  display_name: string;
  lan_ip: string;
  listen_port: number;
  seen_at: number;
  capabilities?: string[];
};

export type DesktopLlmContextProbePayload = {
  provider: string;
  base_url: string;
  api_key?: string;
  model: string;
  timeout_s?: number;
};

export type DesktopTtsVoicesProbePayload = DesktopLlmContextProbePayload;

export type DesktopVirtualReplayLog = {
  id: string;
  name: string;
  enabled: boolean;
  format: string;
  user_rounds: number;
  size_bytes: number;
  uploaded_at: string;
};

export type DesktopLlmConfig = {
  default: string;
  default_embedding?: string;
  default_asr?: string;
  default_tts?: string;
  default_image?: string;
  default_video?: string;
  models: Record<string, Record<string, unknown>>;
};

export type DesktopSettingsData = {
  workspace_root: string;
  python_path?: string;
  python_path_valid?: boolean;
  pip_path?: string;
  pip_path_valid?: boolean;
  git_path?: string;
  git_path_valid?: boolean;
  rg_path?: string;
  rg_path_valid?: boolean;
  python_runtime_mode?: 'auto' | 'system' | 'custom' | string;
  container_roots: DesktopContainerRoot[];
  container_mounts?: DesktopContainerMount[];
  language: string;
  supported_languages: string[];
  llm: DesktopLlmConfig;
  lan_mesh: DesktopLanMeshSettings;
  updated_at: number;
};

export type DesktopDirectoryEntry = {
  name: string;
  path: string;
  entry_type?: 'dir' | 'file';
};

export type DesktopDirectoryListData = {
  current_path: string;
  parent_path: string | null;
  roots: string[];
  items: DesktopDirectoryEntry[];
};

export type DesktopResolvedWorkspacePathData = {
  path: string;
  container_id: number;
  absolute_path: string;
  exists: boolean;
};

export type DesktopSeedJobProgress = {
  percent: number;
  processed_files: number;
  total_files: number;
  processed_bytes: number;
  total_bytes: number;
  speed_bps: number;
  eta_seconds: number | null;
};

export type DesktopSeedJob = {
  job_id: string;
  container_id: number;
  local_root: string;
  cloud_workspace_id: string;
  remote_api_base: string;
  stage: string;
  status: string;
  progress: DesktopSeedJobProgress;
  current_item?: string;
  error?: string;
  created_at: number;
  updated_at: number;
  started_at?: number | null;
  finished_at?: number | null;
};

export type DesktopSeedStartPayload = {
  container_id: number;
  access_token: string;
  local_root?: string;
  remote_api_base?: string;
  cloud_workspace_id?: string;
};

export type DesktopSeedControlPayload = {
  job_id: string;
  action: 'pause' | 'resume' | 'cancel';
};

export type DesktopResetWorkStateSession = {
  agent_id: string;
  session_id: string;
};

export type DesktopResetWorkStateSummary = {
  cancelled_sessions: number;
  cancelled_tasks: number;
  cancelled_team_runs: number;
  cleared_workspaces: number;
  removed_workspace_entries: number;
  fresh_main_sessions: DesktopResetWorkStateSession[];
};

const desktopApi = axios.create({
  timeout: 30000
});

let desktopSettingsInFlight: { token: string; request: Promise<AxiosResponse> } | null = null;
let desktopSettingsCache: { token: string; expiresAt: number; response: AxiosResponse } | null = null;
let desktopSettingsVersion = 0;

const DESKTOP_SETTINGS_CACHE_MS = 500;

const buildDesktopHeaders = (): Record<string, string> => {
  const token = getDesktopLocalToken();
  if (!token) {
    return {};
  }
  return {
    'x-api-key': token,
    Authorization: `Bearer ${token}`
  };
};

export const fetchDesktopSettings = (): Promise<AxiosResponse> => {
  const token = getDesktopLocalToken();
  if (
    desktopSettingsCache &&
    desktopSettingsCache.token === token &&
    desktopSettingsCache.expiresAt > Date.now()
  ) {
    return Promise.resolve(desktopSettingsCache.response);
  }
  if (desktopSettingsInFlight?.token === token) {
    return desktopSettingsInFlight.request;
  }
  const requestVersion = desktopSettingsVersion;
  const request = desktopApi.get('/wunder/desktop/settings', {
    headers: buildDesktopHeaders()
  });
  desktopSettingsInFlight = { token, request };
  void request.then(
    (response) => {
      if (requestVersion === desktopSettingsVersion) {
        desktopSettingsCache = {
          token,
          expiresAt: Date.now() + DESKTOP_SETTINGS_CACHE_MS,
          response
        };
      }
      if (desktopSettingsInFlight?.request === request) {
        desktopSettingsInFlight = null;
      }
    },
    () => {
      if (desktopSettingsInFlight?.request === request) {
        desktopSettingsInFlight = null;
      }
    }
  );
  return request;
};

export const updateDesktopSettings = (payload: ApiPayload) => {
  desktopSettingsVersion += 1;
  desktopSettingsCache = null;
  return desktopApi.put('/wunder/desktop/settings', payload, {
    headers: buildDesktopHeaders()
  });
};

export const resetDesktopWorkState = () =>
  desktopApi.post('/wunder/desktop/reset_work_state', undefined, {
    headers: buildDesktopHeaders()
  });

export const probeDesktopLlmContextWindow = (payload: DesktopLlmContextProbePayload) =>
  desktopApi.post('/wunder/desktop/llm/context_window', payload, {
    headers: buildDesktopHeaders()
  });

export const probeDesktopTtsVoices = (payload: DesktopTtsVoicesProbePayload) =>
  desktopApi.post('/wunder/desktop/llm/tts_voices', payload, {
    headers: buildDesktopHeaders()
  });

export const listDesktopVirtualReplayLogs = () =>
  desktopApi.get('/wunder/desktop/llm/virtual_logs', {
    headers: buildDesktopHeaders()
  });

export const uploadDesktopVirtualReplayLog = (file: File, name?: string) => {
  const form = new FormData();
  form.append('file', file);
  form.append('name', String(name || file.name || '').trim());
  return desktopApi.post('/wunder/desktop/llm/virtual_logs', form, {
    headers: buildDesktopHeaders(),
    timeout: 120000
  });
};

export const setDesktopVirtualReplayLogEnabled = (logId: string, enabled: boolean) =>
  desktopApi.post(
    `/wunder/desktop/llm/virtual_logs/${encodeURIComponent(logId)}`,
    { enabled },
    {
      headers: buildDesktopHeaders()
    }
  );

export const deleteDesktopVirtualReplayLog = (logId: string) =>
  desktopApi.delete(`/wunder/desktop/llm/virtual_logs/${encodeURIComponent(logId)}`, {
    headers: buildDesktopHeaders()
  });

export const listDesktopDirectories = (
  path?: string,
  options?: { includeFiles?: boolean; fileNames?: string[] }
) =>
  desktopApi.get('/wunder/desktop/fs/list', {
    headers: buildDesktopHeaders(),
    params: {
      ...(path && path.trim() ? { path: path.trim() } : {}),
      ...(options?.includeFiles ? { include_files: true } : {}),
      ...(options?.fileNames?.length ? { file_names: options.fileNames.join(',') } : {})
    }
  });

export const startDesktopSeedJob = (payload: DesktopSeedStartPayload) =>
  desktopApi.post('/wunder/desktop/sync/seed/start', payload, {
    headers: buildDesktopHeaders()
  });

export const listDesktopSeedJobs = (params?: { container_id?: number; limit?: number }) =>
  desktopApi.get('/wunder/desktop/sync/seed/jobs', {
    headers: buildDesktopHeaders(),
    params
  });

export const getDesktopSeedJob = (jobId: string) =>
  desktopApi.get(`/wunder/desktop/sync/seed/jobs/${encodeURIComponent(jobId)}`, {
    headers: buildDesktopHeaders()
  });

export const controlDesktopSeedJob = (payload: DesktopSeedControlPayload) =>
  desktopApi.post('/wunder/desktop/sync/seed/control', payload, {
    headers: buildDesktopHeaders()
  });

export const listDesktopLanPeers = () =>
  desktopApi.get('/wunder/desktop/lan/peers', {
    headers: buildDesktopHeaders()
  });

export const resolveDesktopWorkspacePath = (path: string, containerId?: number | null) =>
  desktopApi.get('/wunder/desktop/workspace/resolve_path', {
    headers: buildDesktopHeaders(),
    params: {
      path,
      ...(containerId !== null && containerId !== undefined ? { container_id: containerId } : {})
    }
  });
