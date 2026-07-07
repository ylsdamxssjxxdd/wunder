type ChatDebugEntry = {
  time: string;
  scope: string;
  event: string;
  payload?: unknown;
};

const DEBUG_STORAGE_KEYS = ['wunder:chat-debug', 'wunder_chat_debug', '__wunder_chat_debug__'];
const DEBUG_VERBOSE_STORAGE_KEYS = ['wunder:chat-debug-verbose', 'wunder_chat_debug_verbose'];
const DEBUG_TRUE_VALUES = new Set(['1', 'true', 'on', 'yes', 'debug']);
const DEBUG_HISTORY_KEY = '__WUNDER_CHAT_DEBUG_LOGS__';
const DEBUG_DUMP_FN_KEY = '__wunderChatDebugDump';
const DEBUG_CLEAR_FN_KEY = '__wunderChatDebugClear';
const DEBUG_ENABLE_FN_KEY = '__wunderChatDebugEnable';
const DEBUG_DISABLE_FN_KEY = '__wunderChatDebugDisable';
const DEBUG_ENABLE_VERBOSE_FN_KEY = '__wunderChatDebugEnableVerbose';
const DEBUG_DISABLE_VERBOSE_FN_KEY = '__wunderChatDebugDisableVerbose';
const DEBUG_STATUS_FN_KEY = '__wunderChatDebugStatus';
const DEBUG_MAX_HISTORY = 2000;
const DEBUG_CONSOLE_PAYLOAD_MAX_CHARS = 1200;
const DEBUG_HISTORY_ONLY_SCOPES = new Set([
  'chat.stream.perf'
]);
const DEBUG_HEAVY_CONSOLE_SCOPES = new Set([
  'chat.runtime.shadow',
  'chat.runtime.render',
  'chat.store.terminal-debug'
]);
const DEBUG_VERBOSE_SCOPES = new Set([
  'chat.store.preload',
  'chat.llm.request',
  'chat.store.runtime',
  'chat.compaction.event',
  'chat.compaction.hydrate',
  'chat.compaction.manual',
  'chat.store.loading',
  'chat.store.controller-recovery',
  'messenger.viewport',
  'messenger.workflow-shell',
  'messenger.workflow-surface',
  'chat.composer',
  'messenger.order',
  'messenger.hydration',
  'messenger.virtual'
]);
const DEBUG_ALWAYS_SCOPES = new Set([
  'messenger.conversation',
  'messenger.send',
  'messenger.busy',
  'chat.store.detail',
  'messenger.interaction-blocker',
  'chat.store.busy'
]);
const DEBUG_VERBOSE_SCOPE_EVENTS = new Map<string, Set<string>>([
  [
    'chat.store.runtime',
    new Set([
      'workflow-tool-model-usage',
      'realtime-workflow-mutation'
    ])
  ],
  [
    'messenger.busy',
    new Set([
      'snapshot-change'
    ])
  ],
  [
    'chat.store.detail',
    new Set([
      'foreground-sync-decision',
      'foreground-sync-preserve-running-gap',
      'foreground-sync-keep-live',
      'foreground-sync-replace-live',
      'idle-stream-state-cleared'
    ])
  ]
]);

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

const readVerboseStorageFlag = (): boolean => {
  if (typeof window === 'undefined') return false;
  try {
    for (const key of DEBUG_VERBOSE_STORAGE_KEYS) {
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
export const isChatDebugVerboseEnabled = (): boolean => readVerboseStorageFlag();

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

const setDebugVerboseStorageFlag = (enabled: boolean) => {
  if (typeof window === 'undefined') return;
  try {
    if (enabled) {
      window.localStorage.setItem(DEBUG_VERBOSE_STORAGE_KEYS[0], '1');
      return;
    }
    DEBUG_VERBOSE_STORAGE_KEYS.forEach((key) => {
      window.localStorage.removeItem(key);
    });
  } catch {
    // ignore storage access failures
  }
};

const readDebugHistory = (): ChatDebugEntry[] => {
  if (typeof window === 'undefined') return [];
  const target = window as unknown as Record<string, unknown>;
  return Array.isArray(target[DEBUG_HISTORY_KEY])
    ? (target[DEBUG_HISTORY_KEY] as ChatDebugEntry[])
    : [];
};

const buildDebugStatus = () => ({
  enabled: isChatDebugEnabled(),
  verbose: isChatDebugVerboseEnabled(),
  historyCount: readDebugHistory().length,
  storageKey: DEBUG_STORAGE_KEYS[0],
  verboseStorageKey: DEBUG_VERBOSE_STORAGE_KEYS[0],
  dump: `${DEBUG_DUMP_FN_KEY}()`,
  clear: `${DEBUG_CLEAR_FN_KEY}()`,
  verboseEnable: `${DEBUG_ENABLE_VERBOSE_FN_KEY}()`
});

const announceDebugStatus = (event: string) => {
  if (typeof console === 'undefined') return;
  console.info(`[wunder-chat-debug] ${event}`, buildDebugStatus());
};

const shouldLogScopeEvent = (scope: string, event: string): boolean => {
  const normalizedScope = String(scope || '').trim() || 'unknown';
  const normalizedEvent = String(event || '').trim() || 'event';
  if (DEBUG_ALWAYS_SCOPES.has(normalizedScope)) {
    const suppressedEvents = DEBUG_VERBOSE_SCOPE_EVENTS.get(normalizedScope);
    if (!suppressedEvents) {
      return true;
    }
    if (isChatDebugVerboseEnabled()) {
      return true;
    }
    return !suppressedEvents.has(normalizedEvent);
  }
  if (isChatDebugVerboseEnabled()) {
    return true;
  }
  return !DEBUG_VERBOSE_SCOPES.has(normalizedScope);
};

const ensureDebugAccessors = () => {
  if (typeof window === 'undefined') return;
  const target = window as unknown as Record<string, unknown>;
  target[DEBUG_DUMP_FN_KEY] = () => {
    return JSON.stringify(readDebugHistory(), null, 2);
  };
  target[DEBUG_CLEAR_FN_KEY] = () => {
    const entries = readDebugHistory();
    const cleared = entries.length;
    target[DEBUG_HISTORY_KEY] = [];
    return cleared;
  };
  target[DEBUG_ENABLE_FN_KEY] = () => {
    setDebugStorageFlag(true);
    announceDebugStatus('enabled');
    return true;
  };
  target[DEBUG_DISABLE_FN_KEY] = () => {
    setDebugStorageFlag(false);
    announceDebugStatus('disabled');
    return false;
  };
  target[DEBUG_ENABLE_VERBOSE_FN_KEY] = () => {
    setDebugStorageFlag(true);
    setDebugVerboseStorageFlag(true);
    announceDebugStatus('verbose enabled');
    return true;
  };
  target[DEBUG_DISABLE_VERBOSE_FN_KEY] = () => {
    setDebugVerboseStorageFlag(false);
    announceDebugStatus('verbose disabled');
    return false;
  };
  target[DEBUG_STATUS_FN_KEY] = () => buildDebugStatus();
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

const estimateDebugPayloadSize = (payload: unknown): number => {
  if (payload === undefined) return 0;
  if (typeof payload === 'string') return payload.length;
  try {
    return JSON.stringify(payload).length;
  } catch {
    return String(payload).length;
  }
};

const shouldPrintDebugPayloadToConsole = (scope: string, payload: unknown): boolean => {
  if (payload === undefined) return false;
  if (DEBUG_HISTORY_ONLY_SCOPES.has(scope)) return false;
  if (DEBUG_HEAVY_CONSOLE_SCOPES.has(scope)) return false;
  return estimateDebugPayloadSize(payload) <= DEBUG_CONSOLE_PAYLOAD_MAX_CHARS;
};

const buildDebugPayloadOmissionMeta = (scope: string, payload: unknown): Record<string, unknown> => {
  const payloadType = Array.isArray(payload) ? 'array' : typeof payload;
  if (DEBUG_HEAVY_CONSOLE_SCOPES.has(scope)) {
    return { payloadOmitted: true, payloadType };
  }
  if (DEBUG_HISTORY_ONLY_SCOPES.has(scope)) {
    return { payloadOmitted: true, payloadType, historyOnly: true };
  }
  return {
    payloadOmitted: true,
    payloadType,
    payloadSize: estimateDebugPayloadSize(payload)
  };
};

export const chatDebugLog = (scope: string, event: string, payload?: unknown): void => {
  if (!isChatDebugEnabled()) return;
  const normalizedScope = String(scope || '').trim() || 'unknown';
  const normalizedEvent = String(event || '').trim() || 'event';
  if (!shouldLogScopeEvent(normalizedScope, normalizedEvent)) return;
  const time = new Date().toISOString();
  const entry: ChatDebugEntry = {
    time,
    scope: normalizedScope,
    event: normalizedEvent
  };
  if (payload !== undefined) {
    entry.payload = payload;
  }
  pushDebugEntry(entry);
  const prefix = `[wunder-chat-debug][${entry.time}][${entry.scope}] ${entry.event}`;
  if (payload === undefined) {
    console.info(prefix);
    return;
  }
  if (!shouldPrintDebugPayloadToConsole(normalizedScope, payload)) {
    console.info(prefix, buildDebugPayloadOmissionMeta(normalizedScope, payload));
    return;
  }
  console.info(prefix, payload);
};

ensureDebugAccessors();
