import { writePersistentAccessToken } from '@/utils/authTokenStorage';

const DESKTOP_MODE = 'desktop';
const DESKTOP_BOOTSTRAP_PATH = '/wunder/desktop/bootstrap';
const DESKTOP_TOOL_CALL_MODE_KEY = 'wunder_desktop_tool_call_mode';
const DESKTOP_USER_ID_KEY = 'wunder_desktop_user_id';
const DESKTOP_LOCAL_TOKEN_KEY = 'wunder_desktop_local_token';

export const DESKTOP_TOOL_CALL_MODES = ['tool_call', 'function_call', 'freeform_call'] as const;

export type DesktopToolCallMode = (typeof DESKTOP_TOOL_CALL_MODES)[number];

export type DesktopRuntimeCapabilities = {
  embedded_mode: boolean;
  thread_runtime_active: boolean;
  mission_runtime_active: boolean;
  gateway_maintenance_active: boolean;
  channels_enabled: boolean;
  channel_outbox_worker_enabled: boolean;
  cron_active: boolean;
  lan_overlay_supported: boolean;
  safe_mode?: boolean;
};

export type DesktopRuntime = {
  mode: string;
  runtime_profile?: string;
  runtime_capabilities?: DesktopRuntimeCapabilities;
  safe_mode?: boolean;
  bind_addr: string;
  web_base: string;
  api_base: string;
  ws_base: string;
  token: string;
  desktop_token: string;
  user_id: string;
  app_dir: string;
  workspace_root: string;
  container_roots?: Array<{ container_id: number; root: string }>;
  temp_root: string;
  settings_path: string;
  repo_root: string;
  frontend_root?: string;
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

const asString = (value: unknown): string => (typeof value === 'string' ? value.trim() : '');

const asBoolean = (value: unknown): boolean => Boolean(value);

const normalizeRuntimeCapabilities = (value: unknown): DesktopRuntimeCapabilities | undefined => {
  const source = asRecord(value);
  if (!Object.keys(source).length) {
    return undefined;
  }
  return {
    embedded_mode: asBoolean(source.embedded_mode),
    thread_runtime_active: asBoolean(source.thread_runtime_active),
    mission_runtime_active: asBoolean(source.mission_runtime_active),
    gateway_maintenance_active: asBoolean(source.gateway_maintenance_active),
    channels_enabled: asBoolean(source.channels_enabled),
    channel_outbox_worker_enabled: asBoolean(source.channel_outbox_worker_enabled),
    cron_active: asBoolean(source.cron_active),
    lan_overlay_supported: asBoolean(source.lan_overlay_supported),
    safe_mode: asBoolean(source.safe_mode)
  };
};

const normalizeToolCallMode = (value: unknown): DesktopToolCallMode => {
  const normalized = asString(value).toLowerCase();
  if (!normalized) {
    return 'function_call';
  }
  if (normalized === 'freeform_call' || normalized === 'freeform') {
    return 'freeform_call';
  }
  if (normalized === 'function_call') {
    return 'function_call';
  }
  if (normalized === 'tool_call') {
    return 'tool_call';
  }
  return 'function_call';
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
    safe_mode: asBoolean(source.safe_mode)
  };
  const containerRoots = Array.isArray(source.container_roots)
    ? source.container_roots
        .map((item) => {
          const record = asRecord(item);
          const containerId = Number.parseInt(asString(record.container_id ?? record.containerId), 10);
          const root = asString(record.root);
          if (!Number.isFinite(containerId) || !root) return null;
          return { container_id: containerId, root };
        })
        .filter((item): item is { container_id: number; root: string } => Boolean(item))
    : [];
  if (containerRoots.length) {
    runtime.container_roots = containerRoots;
  }
  const runtimeProfile = asString(source.runtime_profile);
  if (runtimeProfile) {
    runtime.runtime_profile = runtimeProfile;
  }
  const runtimeCapabilities = normalizeRuntimeCapabilities(source.runtime_capabilities);
  if (runtimeCapabilities) {
    runtime.runtime_capabilities = runtimeCapabilities;
  }
  const frontendRoot = asString(source.frontend_root);
  if (frontendRoot) {
    runtime.frontend_root = frontendRoot;
  }
  console.info('[desktop-debug][bootstrap]', {
    mode: runtime.mode,
    workspace_root: runtime.workspace_root,
    app_dir: runtime.app_dir,
    repo_root: runtime.repo_root,
    frontend_root: runtime.frontend_root,
    container_roots: runtime.container_roots || []
  });
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
    try {
      localStorage.removeItem('wunder_desktop_remote_api_base');
    } catch {
      // Ignore stale compatibility key cleanup failures.
    }
    if (runtime.token) {
      writePersistentAccessToken(runtime.token);
    }
  } catch {
    // Ignore localStorage write failures (private mode or quota issues).
  }
};

const ensureDesktopDefaultToolCallMode = (): void => {
  try {
    const stored = localStorage.getItem(DESKTOP_TOOL_CALL_MODE_KEY);
    if (!stored) {
      localStorage.setItem(DESKTOP_TOOL_CALL_MODE_KEY, 'function_call');
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

export const isDesktopSafeModeEnabled = (): boolean =>
  Boolean(getDesktopRuntime()?.safe_mode);

export const isDesktopLocalModeEnabled = (): boolean => isDesktopModeEnabled();

export const reportDesktopRendererStage = (
  stage: string,
  payload: Record<string, unknown> = {}
): void => {
  const normalizedStage = String(stage || '').trim();
  if (!normalizedStage || typeof window === 'undefined') {
    return;
  }
  const bridge = (window as Window & {
    wunderDesktop?: {
      reportRendererStage?: (stage: string, payload?: Record<string, unknown>) => Promise<boolean> | boolean;
    };
  }).wunderDesktop;
  if (typeof bridge?.reportRendererStage !== 'function') {
    return;
  }
  void Promise.resolve(bridge.reportRendererStage(normalizedStage, payload)).catch(() => undefined);
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
    return 'function_call';
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
  // Desktop now uses per-model tool_call_mode from llm.models[*] config.
  // Do not inject a global override into each chat request.
  return null;
};

declare global {
  interface Window {
    __WUNDER_DESKTOP_RUNTIME__?: Record<string, unknown>;
  }
}

export {};
