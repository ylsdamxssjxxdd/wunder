import api from './http';

import type { ApiPayload } from './types';

export type DesktopContainerRoot = {
  container_id: number;
  root: string;
};

export type DesktopRemoteGatewaySettings = {
  enabled: boolean;
  server_base_url: string;
  api_key: string;
  role_name: string;
  use_remote_sandbox: boolean;
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

export const fetchDesktopSettings = () => api.get('/desktop/settings');

export const updateDesktopSettings = (payload: ApiPayload) => api.put('/desktop/settings', payload);
