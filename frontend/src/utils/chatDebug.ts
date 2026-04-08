type ChatDebugEntry = {
  time: string;
  scope: string;
  event: string;
  payload?: unknown;
};

const DEBUG_STORAGE_KEYS = ['wunder:chat-debug', 'wunder_chat_debug', '__wunder_chat_debug__'];
const DEBUG_TRUE_VALUES = new Set(['1', 'true', 'on', 'yes', 'debug']);
const DEBUG_HISTORY_KEY = '__WUNDER_CHAT_DEBUG_LOGS__';
const DEBUG_DUMP_FN_KEY = '__wunderChatDebugDump';
const DEBUG_CLEAR_FN_KEY = '__wunderChatDebugClear';
const DEBUG_ENABLE_FN_KEY = '__wunderChatDebugEnable';
const DEBUG_DISABLE_FN_KEY = '__wunderChatDebugDisable';
const DEBUG_MAX_HISTORY = 2000;

const readStorageFlag = (): boolean => {
  if (typeof window === 'undefined') return false;
  try {
    for (const key of DEBUG_STORAGE_KEYS) {
      const raw = String(window.localStorage.getItem(key) || '')
        .trim()
        .toLowerCase();
      if (DEBUG_TRUE_VALUES.has(raw)) {
        return true;
      }
    }
  } catch {
    // ignore storage access failures
  }
  return false;
};

const readSearchFlag = (): boolean => {
  if (typeof window === 'undefined') return false;
  try {
    const params = new URLSearchParams(window.location.search || '');
    const raw = String(params.get('chat_debug') || params.get('chatDebug') || '')
      .trim()
      .toLowerCase();
    return DEBUG_TRUE_VALUES.has(raw);
  } catch {
    return false;
  }
};

export const isChatDebugEnabled = (): boolean => readStorageFlag() || readSearchFlag();

const setDebugStorageFlag = (enabled: boolean) => {
  if (typeof window === 'undefined') return;
  try {
    if (enabled) {
      window.localStorage.setItem(DEBUG_STORAGE_KEYS[0], '1');
      return;
    }
    DEBUG_STORAGE_KEYS.forEach((key) => {
      window.localStorage.removeItem(key);
    });
  } catch {
    // ignore storage access failures
  }
};

const ensureDebugAccessors = () => {
  if (typeof window === 'undefined') return;
  const target = window as unknown as Record<string, unknown>;
  if (typeof target[DEBUG_DUMP_FN_KEY] === 'function') {
    return;
  }
  target[DEBUG_DUMP_FN_KEY] = () => {
    const entries = Array.isArray(target[DEBUG_HISTORY_KEY]) ? target[DEBUG_HISTORY_KEY] : [];
    return JSON.stringify(entries, null, 2);
  };
  target[DEBUG_CLEAR_FN_KEY] = () => {
    const entries = Array.isArray(target[DEBUG_HISTORY_KEY]) ? target[DEBUG_HISTORY_KEY] : [];
    const cleared = entries.length;
    target[DEBUG_HISTORY_KEY] = [];
    return cleared;
  };
  target[DEBUG_ENABLE_FN_KEY] = () => {
    setDebugStorageFlag(true);
    return true;
  };
  target[DEBUG_DISABLE_FN_KEY] = () => {
    setDebugStorageFlag(false);
    return false;
  };
};

const pushDebugEntry = (entry: ChatDebugEntry) => {
  if (typeof window === 'undefined') return;
  const target = window as unknown as Record<string, unknown>;
  const entries = Array.isArray(target[DEBUG_HISTORY_KEY])
    ? (target[DEBUG_HISTORY_KEY] as ChatDebugEntry[])
    : [];
  entries.push(entry);
  if (entries.length > DEBUG_MAX_HISTORY) {
    entries.splice(0, entries.length - DEBUG_MAX_HISTORY);
  }
  target[DEBUG_HISTORY_KEY] = entries;
  ensureDebugAccessors();
};

export const chatDebugLog = (scope: string, event: string, payload?: unknown): void => {
  if (!isChatDebugEnabled()) return;
  const time = new Date().toISOString();
  const entry: ChatDebugEntry = {
    time,
    scope: String(scope || '').trim() || 'unknown',
    event: String(event || '').trim() || 'event'
  };
  if (payload !== undefined) {
    entry.payload = payload;
  }
  pushDebugEntry(entry);
  const prefix = `[wunder-chat-debug][${entry.time}][${entry.scope}] ${entry.event}`;
  if (payload === undefined) {
    console.debug(prefix);
    return;
  }
  console.debug(prefix, payload);
};

ensureDebugAccessors();
