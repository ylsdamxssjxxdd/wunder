const DEFAULT_FALLBACK_BASE = 'http://localhost:18000/wunder';

let cachedConfig = null;
let loadPromise = null;

const normalizeConfig = (raw) => {
  if (!raw || typeof raw !== 'object') {
    return {};
  }
  const apiBase =
    raw.api_base ||
    raw.apiBase ||
    raw.api_base_url ||
    raw.apiBaseUrl ||
    raw.api_url ||
    raw.apiUrl ||
    '';
  return {
    api_base: typeof apiBase === 'string' ? apiBase.trim() : ''
  };
};

export const loadRuntimeConfig = async () => {
  if (loadPromise) {
    return loadPromise;
  }
  loadPromise = (async () => {
    try {
      const response = await fetch('/config.json', { cache: 'no-store' });
      if (!response.ok) {
        cachedConfig = {};
        return cachedConfig;
      }
      const payload = await response.json();
      cachedConfig = normalizeConfig(payload);
      return cachedConfig;
    } catch (error) {
      cachedConfig = {};
      return cachedConfig;
    }
  })();
  return loadPromise;
};

export const getRuntimeConfig = () => cachedConfig || {};

export const resolveApiBase = () => {
  const runtime = cachedConfig?.api_base;
  if (runtime) {
    return runtime;
  }
  const envBase = import.meta.env.VITE_API_BASE_URL || import.meta.env.VITE_API_BASE;
  return envBase || DEFAULT_FALLBACK_BASE;
};
