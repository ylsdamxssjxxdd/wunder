import axios from 'axios';

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

export type DesktopRemoteGatewaySettings = {
  enabled: boolean;
  server_base_url: string;
};

export type DesktopLlmConfig = {
  default: string;
  models: Record<string, Record<string, unknown>>;
};

export type DesktopSettingsData = {
  workspace_root: string;
  container_roots: DesktopContainerRoot[];
  container_mounts?: DesktopContainerMount[];
  language: string;
  supported_languages: string[];
  llm: DesktopLlmConfig;
  remote_gateway: DesktopRemoteGatewaySettings;
  updated_at: number;
};

export type DesktopDirectoryEntry = {
  name: string;
  path: string;
};

export type DesktopDirectoryListData = {
  current_path: string;
  parent_path: string | null;
  roots: string[];
  items: DesktopDirectoryEntry[];
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

const desktopApi = axios.create({
  timeout: 30000
});

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

export const fetchDesktopSettings = () =>
  desktopApi.get('/wunder/desktop/settings', {
    headers: buildDesktopHeaders()
  });

export const updateDesktopSettings = (payload: ApiPayload) =>
  desktopApi.put('/wunder/desktop/settings', payload, {
    headers: buildDesktopHeaders()
  });

export const listDesktopDirectories = (path?: string) =>
  desktopApi.get('/wunder/desktop/fs/list', {
    headers: buildDesktopHeaders(),
    params: path && path.trim() ? { path: path.trim() } : undefined
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
