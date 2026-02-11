import { getDesktopRuntime, getDesktopRemoteApiBaseOverride } from '@/config/desktop';

const DEFAULT_FALLBACK_BASE = '/wunder';

type RuntimeConfig = {
  api_base: string;
  ws_base: string;
  token: string;
  user_id: string;
  mode: string;
  workspace_root: string;
};

const EMPTY_RUNTIME: RuntimeConfig = {
  api_base: '',
  ws_base: '',
  token: '',
  user_id: '',
  mode: '',
  workspace_root: ''
};

let cachedConfig: RuntimeConfig | null = null;
let loadPromise: Promise<RuntimeConfig> | null = null;

const asRecord = (raw: unknown): Record<string, unknown> =>
  raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {};

const asString = (value: unknown): string => (typeof value === 'string' ? value.trim() : '');

const normalizeConfig = (raw: unknown): RuntimeConfig => {
  const source = asRecord(raw);
  const apiBase =
    source.api_base ||
    source.apiBase ||
    source.api_base_url ||
    source.apiBaseUrl ||
    source.api_url ||
    source.apiUrl ||
    '';
  return {
    api_base: asString(apiBase),
    ws_base: asString(source.ws_base || source.wsBase),
    token: asString(source.token),
    user_id: asString(source.user_id || source.userId),
    mode: asString(source.mode),
    workspace_root: asString(source.workspace_root || source.workspaceRoot)
  };
};

const mergeDesktopFallback = (runtime: RuntimeConfig): RuntimeConfig => {
  const desktopRuntime = getDesktopRuntime();
  if (!desktopRuntime) {
    return runtime;
  }
  return {
    api_base: runtime.api_base || desktopRuntime.api_base,
    ws_base: runtime.ws_base || desktopRuntime.ws_base,
    token: runtime.token || desktopRuntime.token,
    user_id: runtime.user_id || desktopRuntime.user_id,
    mode: runtime.mode || desktopRuntime.mode,
    workspace_root: runtime.workspace_root || desktopRuntime.workspace_root
  };
};

export const loadRuntimeConfig = async (): Promise<RuntimeConfig> => {
  if (loadPromise) {
    return loadPromise;
  }
  loadPromise = (async () => {
    try {
      const response = await fetch('/config.json', { cache: 'no-store' });
      if (!response.ok) {
        cachedConfig = mergeDesktopFallback({ ...EMPTY_RUNTIME });
        return cachedConfig;
      }
      const payload = await response.json();
      cachedConfig = mergeDesktopFallback(normalizeConfig(payload));
      return cachedConfig;
    } catch {
      cachedConfig = mergeDesktopFallback({ ...EMPTY_RUNTIME });
      return cachedConfig;
    }
  })();
  return loadPromise;
};

export const getRuntimeConfig = (): RuntimeConfig => cachedConfig || { ...EMPTY_RUNTIME };

export const resolveApiBase = (): string => {
  const remoteOverride = getDesktopRemoteApiBaseOverride();
  if (remoteOverride) {
    return remoteOverride;
  }

  const desktopRuntime = getDesktopRuntime();
  if (desktopRuntime) {
    const localWebBase = (desktopRuntime.web_base || window.location.origin || '').replace(/\/+$/, '');
    if (localWebBase) {
      return `${localWebBase}/wunder`;
    }
  }

  const runtime = cachedConfig?.api_base;
  if (runtime) {
    return runtime;
  }
  const envBase = import.meta.env.VITE_API_BASE_URL || import.meta.env.VITE_API_BASE;
  return envBase || DEFAULT_FALLBACK_BASE;
};
