const COMMAND_STREAM_VISUALIZATION_STORAGE_KEY = 'wunder_command_stream_visualization';

const normalizeFlag = (value: unknown): boolean => {
  const text = String(value ?? '').trim().toLowerCase();
  return text === '1' || text === 'true' || text === 'yes' || text === 'enabled' || text === 'on';
};

export const isCommandStreamVisualizationEnabled = (): boolean => {
  const env = (import.meta as { env?: Record<string, unknown> }).env || {};
  if (normalizeFlag(env.VITE_COMMAND_STREAM_VISUALIZATION)) {
    return true;
  }
  if (typeof window === 'undefined') {
    return false;
  }
  try {
    return normalizeFlag(window.localStorage?.getItem(COMMAND_STREAM_VISUALIZATION_STORAGE_KEY));
  } catch {
    return false;
  }
};

export const isCommandStreamRuntimeEvent = (eventType: unknown): boolean => {
  const normalized = String(eventType ?? '').trim().toLowerCase();
  return normalized === 'command_session_delta';
};
