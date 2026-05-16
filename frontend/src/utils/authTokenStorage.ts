export const ACCESS_TOKEN_STORAGE_KEY = 'access_token';
const ACCESS_TOKEN_MODE_STORAGE_KEY = 'access_token_mode';
const ACCESS_TOKEN_MODE_SESSION = 'session';

const readStorageToken = (storage: Storage | undefined): string => {
  if (!storage) return '';
  try {
    return String(storage.getItem(ACCESS_TOKEN_STORAGE_KEY) || '').trim();
  } catch {
    return '';
  }
};

const resolveSessionStorage = (): Storage | undefined =>
  typeof window === 'undefined' ? undefined : window.sessionStorage;

const resolveLocalStorage = (): Storage | undefined =>
  typeof window === 'undefined' ? undefined : window.localStorage;

export const readSessionAccessToken = (): string => readStorageToken(resolveSessionStorage());

export const readLocalAccessToken = (): string => readStorageToken(resolveLocalStorage());

const readAccessTokenMode = (): string => {
  const storage = resolveSessionStorage();
  if (!storage) return '';
  try {
    return String(storage.getItem(ACCESS_TOKEN_MODE_STORAGE_KEY) || '').trim();
  } catch {
    return '';
  }
};

const writeAccessTokenMode = (mode: string): void => {
  const cleaned = String(mode || '').trim();
  try {
    const storage = resolveSessionStorage();
    if (!storage) return;
    if (!cleaned) {
      storage.removeItem(ACCESS_TOKEN_MODE_STORAGE_KEY);
      return;
    }
    storage.setItem(ACCESS_TOKEN_MODE_STORAGE_KEY, cleaned);
  } catch {
    // ignore storage failures
  }
};

export const readAccessToken = (): string => {
  const mode = readAccessTokenMode();
  if (mode === ACCESS_TOKEN_MODE_SESSION) {
    return readSessionAccessToken();
  }
  return readSessionAccessToken() || readLocalAccessToken();
};

export const hasSessionAccessToken = (): boolean => Boolean(readSessionAccessToken());

export const hasLocalAccessToken = (): boolean => Boolean(readLocalAccessToken());

export const isSessionAccessTokenMode = (): boolean => readAccessTokenMode() === ACCESS_TOKEN_MODE_SESSION;

export const getActiveAccessTokenSource = (): 'session' | 'local' | 'none' => {
  if (readAccessTokenMode() === ACCESS_TOKEN_MODE_SESSION) {
    return hasSessionAccessToken() ? 'session' : 'none';
  }
  if (hasSessionAccessToken()) {
    return 'session';
  }
  if (hasLocalAccessToken()) {
    return 'local';
  }
  return 'none';
};

export const writeSessionAccessToken = (token: string): void => {
  const cleaned = String(token || '').trim();
  if (!cleaned) return;
  try {
    resolveSessionStorage()?.setItem(ACCESS_TOKEN_STORAGE_KEY, cleaned);
    writeAccessTokenMode(ACCESS_TOKEN_MODE_SESSION);
  } catch {
    // ignore storage failures
  }
};

export const writePersistentAccessToken = (token: string): void => {
  const cleaned = String(token || '').trim();
  if (!cleaned) return;
  try {
    resolveSessionStorage()?.removeItem(ACCESS_TOKEN_STORAGE_KEY);
    writeAccessTokenMode('');
  } catch {
    // ignore storage failures
  }
  try {
    resolveLocalStorage()?.setItem(ACCESS_TOKEN_STORAGE_KEY, cleaned);
  } catch {
    // ignore storage failures
  }
};

export const clearAccessToken = (): void => {
  const source = getActiveAccessTokenSource();
  if (source === 'session') {
    try {
      resolveSessionStorage()?.removeItem(ACCESS_TOKEN_STORAGE_KEY);
    } catch {
      // ignore storage failures
    }
    return;
  }
  if (source === 'local') {
    const storage = resolveLocalStorage();
    try {
      storage?.removeItem(ACCESS_TOKEN_STORAGE_KEY);
      writeAccessTokenMode('');
    } catch {
      // ignore storage failures
    }
    return;
  }
  // Keep session mode sticky when the current tab has already been isolated to session storage.
};

export const clearAccessTokenIfCurrent = (token: string): boolean => {
  const expected = String(token || '').trim();
  if (!expected) {
    clearAccessToken();
    return true;
  }
  const mode = readAccessTokenMode();
  if (mode === ACCESS_TOKEN_MODE_SESSION) {
    if (readSessionAccessToken() !== expected) {
      return false;
    }
    try {
      resolveSessionStorage()?.removeItem(ACCESS_TOKEN_STORAGE_KEY);
    } catch {
      // ignore storage failures
    }
    return true;
  }
  if (readSessionAccessToken() === expected) {
    try {
      resolveSessionStorage()?.removeItem(ACCESS_TOKEN_STORAGE_KEY);
    } catch {
      // ignore storage failures
    }
    return true;
  }
  if (readLocalAccessToken() !== expected) {
    return false;
  }
  try {
    resolveLocalStorage()?.removeItem(ACCESS_TOKEN_STORAGE_KEY);
    writeAccessTokenMode('');
  } catch {
    // ignore storage failures
  }
  return true;
};

export const clearSessionAccessToken = (): void => {
  try {
    const storage = resolveSessionStorage();
    storage?.removeItem(ACCESS_TOKEN_STORAGE_KEY);
    storage?.removeItem(ACCESS_TOKEN_MODE_STORAGE_KEY);
  } catch {
    // ignore storage failures
  }
};

export const clearAllAccessTokens = (): void => {
  try {
    resolveSessionStorage()?.removeItem(ACCESS_TOKEN_STORAGE_KEY);
    resolveSessionStorage()?.removeItem(ACCESS_TOKEN_MODE_STORAGE_KEY);
  } catch {
    // ignore storage failures
  }
  try {
    resolveLocalStorage()?.removeItem(ACCESS_TOKEN_STORAGE_KEY);
  } catch {
    // ignore storage failures
  }
};
