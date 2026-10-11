const COMMAND_STREAM_VISUALIZATION_STORAGE_KEY = 'wunder_command_stream_visualization';

const truthyText = (value: unknown): boolean => {
  const text = String(value ?? '').trim().toLowerCase();
  return text === '1' || text === 'true' || text === 'yes' || text === 'enabled' || text === 'on';
};

const falsyText = (value: unknown): boolean => {
  const text = String(value ?? '').trim().toLowerCase();
  return text === '0' || text === 'false' || text === 'no' || text === 'disabled' || text === 'off';
};

/**
 * Command-stream visualization is on by default: the send stream now delivers
 * `command_session_delta` frames and the tool workflow timeline renders live
 * command output from the command-session snapshots. An explicit env/localStorage
 * opt-out keeps a kill switch for debugging.
 */
export const isCommandStreamVisualizationEnabled = (): boolean => {
  const env = (import.meta as { env?: Record<string, unknown> }).env || {};
  if (truthyText(env.VITE_COMMAND_STREAM_VISUALIZATION)) {
    return true;
  }
  if (falsyText(env.VITE_COMMAND_STREAM_VISUALIZATION)) {
    return false;
  }
  if (typeof window === 'undefined') {
    return true;
  }
  try {
    const stored = window.localStorage?.getItem(COMMAND_STREAM_VISUALIZATION_STORAGE_KEY);
    if (stored === null || String(stored).trim() === '') {
      return true;
    }
    if (truthyText(stored)) {
      return true;
    }
    if (falsyText(stored)) {
      return false;
    }
    return true;
  } catch {
    return true;
  }
};

export const isCommandStreamRuntimeEvent = (eventType: unknown): boolean => {
  const normalized = String(eventType ?? '').trim().toLowerCase();
  return normalized === 'command_session_delta';
};
