const DEFAULT_FALLBACK_BASE = 'http://localhost:18000/wunder';

type RuntimeConfig = {
  api_base: string;
};

let cachedConfig: RuntimeConfig | null = null;
let loadPromise: Promise<RuntimeConfig> | null = null;

const asRecord = (raw: unknown): Record<string, unknown> =>
  raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {};

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
    api_base: typeof apiBase === 'string' ? apiBase.trim() : ''
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
        cachedConfig = { api_base: '' };
        return cachedConfig;
      }
      const payload = await response.json();
      cachedConfig = normalizeConfig(payload);
      return cachedConfig;
    } catch {
      cachedConfig = { api_base: '' };
      return cachedConfig;
    }
  })();
  return loadPromise;
};

export const getRuntimeConfig = (): RuntimeConfig => cachedConfig || { api_base: '' };

export const resolveApiBase = (): string => {
  const runtime = cachedConfig?.api_base;
  if (runtime) {
    return runtime;
  }
  const envBase = import.meta.env.VITE_API_BASE_URL || import.meta.env.VITE_API_BASE;
  return envBase || DEFAULT_FALLBACK_BASE;
};
