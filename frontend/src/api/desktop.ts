import axios from 'axios';

import { getDesktopLocalToken } from '@/config/desktop';

import type { ApiPayload } from './types';

export type DesktopContainerRoot = {
  container_id: number;
  root: string;
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
  language: string;
  supported_languages: string[];
  llm: DesktopLlmConfig;
  remote_gateway: DesktopRemoteGatewaySettings;
  updated_at: number;
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
