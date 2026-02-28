const DESKTOP_MODE = 'desktop';
const DESKTOP_BOOTSTRAP_PATH = '/wunder/desktop/bootstrap';
const DESKTOP_TOOL_CALL_MODE_KEY = 'wunder_desktop_tool_call_mode';
const DESKTOP_USER_ID_KEY = 'wunder_desktop_user_id';
const DESKTOP_LOCAL_TOKEN_KEY = 'wunder_desktop_local_token';
const DESKTOP_REMOTE_API_BASE_KEY = 'wunder_desktop_remote_api_base';
const DESKTOP_REMOTE_MODE_CHANGE_EVENT = 'wunder:desktop-remote-mode-change';

export const DESKTOP_TOOL_CALL_MODES = ['tool_call', 'function_call'] as const;

export type DesktopToolCallMode = (typeof DESKTOP_TOOL_CALL_MODES)[number];

export type DesktopRuntime = {
  mode: string;
  bind_addr: string;
  web_base: string;
  api_base: string;
  ws_base: string;
  token: string;
  desktop_token: string;
  user_id: string;
  app_dir: string;
  workspace_root: string;
  temp_root: string;
  settings_path: string;
  repo_root: string;
  frontend_root?: string;
  remote_enabled: boolean;
  remote_connected: boolean;
  remote_server_base_url: string;
  remote_error?: string;
};

export type DesktopRemoteModeChangeDetail = {
  remoteAuthMode: boolean;
  remoteApiBase: string;
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

const asString = (value: unknown): string => (typeof value === 'string' ? value.trim() : '');

const normalizeToolCallMode = (value: unknown): DesktopToolCallMode => {
  const normalized = asString(value).toLowerCase();
  if (normalized === 'function_call') {
    return 'function_call';
  }
  return 'tool_call';
};

const normalizeRemoteApiBase = (raw: string): string => {
  const cleaned = String(raw || '').trim();
  if (!cleaned) {
    return '';
  }

  const withScheme = /^https?:\/\//i.test(cleaned) ? cleaned : `http://${cleaned}`;
  try {
    const parsed = new URL(withScheme);
    if (!/^https?:$/i.test(parsed.protocol)) {
      return '';
    }
    let pathname = parsed.pathname.replace(/\/+$/, '');
    if (!pathname || pathname === '/') {
      pathname = '/wunder';
    } else if (!pathname.endsWith('/wunder')) {
      pathname = `${pathname}/wunder`;
    }
    parsed.pathname = pathname;
    parsed.search = '';
    parsed.hash = '';
    return parsed.toString().replace(/\/$/, '');
  } catch {
    return '';
  }
};

const readRemoteApiBaseOverride = (): string => {
  try {
    return normalizeRemoteApiBase(localStorage.getItem(DESKTOP_REMOTE_API_BASE_KEY) || '');
  } catch {
    return '';
  }
};

const getDesktopRemoteModeDetail = (): DesktopRemoteModeChangeDetail => {
  const remoteApiBase = readRemoteApiBaseOverride();
  return {
    remoteAuthMode: Boolean(remoteApiBase),
    remoteApiBase
  };
};

const emitDesktopRemoteModeChange = (): DesktopRemoteModeChangeDetail => {
  const detail = getDesktopRemoteModeDetail();
  if (typeof window !== 'undefined') {
    window.dispatchEvent(
      new CustomEvent<DesktopRemoteModeChangeDetail>(DESKTOP_REMOTE_MODE_CHANGE_EVENT, {
        detail
      })
    );
  }
  return detail;
};

const writeRemoteApiBaseOverride = (serverBaseUrl: string): string => {
  const normalized = normalizeRemoteApiBase(serverBaseUrl);
  try {
    if (normalized) {
      localStorage.setItem(DESKTOP_REMOTE_API_BASE_KEY, normalized);
    } else {
      localStorage.removeItem(DESKTOP_REMOTE_API_BASE_KEY);
    }
  } catch {
    // ignore localStorage failures
  }
  emitDesktopRemoteModeChange();
  return normalized;
};

const clearRemoteApiBaseOverrideInternal = (): void => {
  try {
    localStorage.removeItem(DESKTOP_REMOTE_API_BASE_KEY);
  } catch {
    // ignore localStorage failures
  }
  emitDesktopRemoteModeChange();
};

const normalizeRuntime = (value: unknown): DesktopRuntime | null => {
  const source = asRecord(value);
  const mode = asString(source.mode).toLowerCase();
  if (mode !== DESKTOP_MODE) {
    return null;
  }
  const runtime: DesktopRuntime = {
    mode,
    bind_addr: asString(source.bind_addr),
    web_base: asString(source.web_base),
    api_base: asString(source.api_base),
    ws_base: asString(source.ws_base),
    token: asString(source.token),
    desktop_token: asString(source.desktop_token || source.desktopToken || source.token),
    user_id: asString(source.user_id),
    app_dir: asString(source.app_dir),
    workspace_root: asString(source.workspace_root),
    temp_root: asString(source.temp_root),
    settings_path: asString(source.settings_path),
    repo_root: asString(source.repo_root),
    remote_enabled: Boolean(source.remote_enabled),
    remote_connected: Boolean(source.remote_connected),
    remote_server_base_url: asString(source.remote_server_base_url)
  };
  const remoteError = asString(source.remote_error);
  if (remoteError) {
    runtime.remote_error = remoteError;
  }
  const frontendRoot = asString(source.frontend_root);
  if (frontendRoot) {
    runtime.frontend_root = frontendRoot;
  }
  return runtime;
};

const readInjectedRuntime = (): DesktopRuntime | null =>
  normalizeRuntime((window as Window).__WUNDER_DESKTOP_RUNTIME__);

let runtimeCache: DesktopRuntime | null = readInjectedRuntime();

const isDesktopShell = (): boolean => {
  const runtimeWindow = window as Window & {
    __TAURI__?: unknown;
    __TAURI_INTERNALS__?: unknown;
    wunderDesktop?: unknown;
  };
  return Boolean(
    runtimeWindow.__TAURI__ || runtimeWindow.__TAURI_INTERNALS__ || runtimeWindow.wunderDesktop
  );
};

const syncDesktopIdentity = (runtime: DesktopRuntime | null): void => {
  if (!runtime || runtime.mode !== DESKTOP_MODE) {
    return;
  }
  try {
    if (runtime.desktop_token) {
      localStorage.setItem(DESKTOP_LOCAL_TOKEN_KEY, runtime.desktop_token);
    }
    if (runtime.user_id) {
      localStorage.setItem(DESKTOP_USER_ID_KEY, runtime.user_id);
    }

    let remoteApiBase = readRemoteApiBaseOverride();
    if (!remoteApiBase && runtime.remote_enabled && runtime.remote_connected) {
      remoteApiBase = writeRemoteApiBaseOverride(runtime.api_base || runtime.remote_server_base_url);
    }

    const remoteAuthMode = Boolean(remoteApiBase);

    if (!remoteAuthMode) {
      if (runtime.token) {
        localStorage.setItem('access_token', runtime.token);
      }
      return;
    }

    const current = String(localStorage.getItem('access_token') || '').trim();
    if (current && runtime.desktop_token && current === runtime.desktop_token) {
      localStorage.removeItem('access_token');
    }
  } catch {
    // Ignore localStorage write failures (private mode or quota issues).
  }
};

const ensureDesktopDefaultToolCallMode = (): void => {
  try {
    const stored = localStorage.getItem(DESKTOP_TOOL_CALL_MODE_KEY);
    if (!stored) {
      localStorage.setItem(DESKTOP_TOOL_CALL_MODE_KEY, 'tool_call');
    }
  } catch {
    // Ignore localStorage write failures.
  }
};

syncDesktopIdentity(runtimeCache);
if (runtimeCache) {
  ensureDesktopDefaultToolCallMode();
}

const parseBootstrapPayload = (payload: unknown): DesktopRuntime | null => {
  const source = asRecord(payload);
  const data = source.data;
  return normalizeRuntime(data);
};

export const initDesktopRuntime = async (): Promise<DesktopRuntime | null> => {
  if (runtimeCache) {
    syncDesktopIdentity(runtimeCache);
    ensureDesktopDefaultToolCallMode();
    return runtimeCache;
  }

  if (!isDesktopShell()) {
    return null;
  }

  try {
    const response = await fetch(DESKTOP_BOOTSTRAP_PATH, { cache: 'no-store' });
    if (!response.ok) {
      return null;
    }
    const payload = await response.json();
    const runtime = parseBootstrapPayload(payload);
    if (!runtime) {
      return null;
    }
    runtimeCache = runtime;
    syncDesktopIdentity(runtimeCache);
    ensureDesktopDefaultToolCallMode();
    return runtimeCache;
  } catch {
    return null;
  }
};

export const isDesktopModeEnabled = (): boolean => {
  if (runtimeCache) {
    return true;
  }
  const runtime = readInjectedRuntime();
  if (!runtime) {
    return false;
  }
  runtimeCache = runtime;
  syncDesktopIdentity(runtimeCache);
  ensureDesktopDefaultToolCallMode();
  return true;
};

export const getDesktopRuntime = (): DesktopRuntime | null => {
  if (!runtimeCache) {
    runtimeCache = readInjectedRuntime();
  }
  return runtimeCache;
};

export const setDesktopRemoteApiBaseOverride = (serverBaseUrl: string): string =>
  writeRemoteApiBaseOverride(serverBaseUrl);

export const clearDesktopRemoteApiBaseOverride = (): void => {
  clearRemoteApiBaseOverrideInternal();
};

export const getDesktopRemoteApiBaseOverride = (): string => readRemoteApiBaseOverride();

export const isDesktopRemoteAuthMode = (): boolean => Boolean(readRemoteApiBaseOverride());

export const onDesktopRemoteModeChange = (
  listener: (detail: DesktopRemoteModeChangeDetail) => void
): (() => void) => {
  if (typeof window === 'undefined') {
    return () => undefined;
  }
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<DesktopRemoteModeChangeDetail>)?.detail;
    if (detail && typeof detail === 'object') {
      listener(detail);
      return;
    }
    listener(getDesktopRemoteModeDetail());
  };
  window.addEventListener(DESKTOP_REMOTE_MODE_CHANGE_EVENT, handler as EventListener);
  return () => {
    window.removeEventListener(DESKTOP_REMOTE_MODE_CHANGE_EVENT, handler as EventListener);
  };
};

export const getDesktopLocalToken = (): string => {
  const runtime = getDesktopRuntime();
  if (runtime?.desktop_token) {
    return runtime.desktop_token;
  }
  try {
    return String(localStorage.getItem(DESKTOP_LOCAL_TOKEN_KEY) || '').trim();
  } catch {
    return '';
  }
};

export const getDesktopToolCallMode = (): DesktopToolCallMode => {
  try {
    return normalizeToolCallMode(localStorage.getItem(DESKTOP_TOOL_CALL_MODE_KEY));
  } catch {
    return 'tool_call';
  }
};

export const setDesktopToolCallMode = (mode: DesktopToolCallMode): void => {
  const normalized = normalizeToolCallMode(mode);
  try {
    localStorage.setItem(DESKTOP_TOOL_CALL_MODE_KEY, normalized);
  } catch {
    // Ignore localStorage write failures.
  }
};

export const getDesktopToolCallModeForRequest = (): DesktopToolCallMode | null => {
  if (!isDesktopModeEnabled()) {
    return null;
  }
  return getDesktopToolCallMode();
};

declare global {
  interface Window {
    __WUNDER_DESKTOP_RUNTIME__?: Record<string, unknown>;
  }
}

export {};
