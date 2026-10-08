const DEMO_MODE_KEY = 'demo_mode';
const DEMO_IDENTITY_KEY = 'demo_identity';
const DEMO_PROFILE_KEY = 'demo_profile';
const DEMO_CHAT_KEY = 'demo_chat_state';
const DEMO_WORKSPACE_KEY = 'demo_workspace_state';
const DEMO_TOKEN_KEY = 'demo_access_token';

type DemoIdentity = {
  demo_id: string;
};

type DemoProfile = {
  id: string;
  username: string;
  access_level: string;
  unit_id: string | null;
  unit: unknown | null;
  is_demo: boolean;
  daily_quota: number;
  daily_quota_used: number;
  daily_quota_date: string;
  created_at: string;
  [key: string]: unknown;
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

const readJson = <T>(key: string, fallback: T): T => {
  if (!key) return fallback;
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
};

const writeJson = <T>(key: string, value: T): void => {
  if (!key) return;
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Ignore storage errors in private mode / quota limit.
  }
};

const normalizeDemoSeed = (value: unknown): string => {
  const raw = String(value || '').trim();
  if (!raw) return '';
  const trimmed = raw.startsWith('demo_') ? raw.slice(5) : raw;
  const cleaned = trimmed.replace(/[^a-zA-Z0-9_-]/g, '');
  return cleaned.slice(0, 24);
};

export const ensureDemoIdentity = (): DemoIdentity => {
  const cached = readJson<DemoIdentity | null>(DEMO_IDENTITY_KEY, null);
  if (cached && cached.demo_id) {
    return cached;
  }
  const cachedProfile = readJson<Record<string, unknown> | null>(DEMO_PROFILE_KEY, null);
  const profileRecord = asRecord(cachedProfile);
  const seed =
    normalizeDemoSeed(profileRecord.username) ||
    normalizeDemoSeed(profileRecord.id) ||
    Math.random().toString(36).slice(2, 10);
  const identity: DemoIdentity = { demo_id: seed };
  writeJson(DEMO_IDENTITY_KEY, identity);
  return identity;
};

export const getDemoToken = (): string => {
  try {
    return localStorage.getItem(DEMO_TOKEN_KEY) || '';
  } catch {
    return '';
  }
};

export const setDemoToken = (token: string): void => {
  if (!token) return;
  try {
    localStorage.setItem(DEMO_TOKEN_KEY, token);
  } catch {
    // Ignore storage errors.
  }
};

export const clearDemoToken = (): void => {
  try {
    localStorage.removeItem(DEMO_TOKEN_KEY);
  } catch {
    // Ignore storage errors.
  }
};

export const enableDemoMode = (): void => {
  try {
    localStorage.setItem(DEMO_MODE_KEY, '1');
  } catch {
    // Ignore storage errors.
  }
};

export const disableDemoMode = (): void => {
  try {
    localStorage.removeItem(DEMO_MODE_KEY);
  } catch {
    // Ignore storage errors.
  }
};

export const isDemoMode = (): boolean => {
  try {
    return localStorage.getItem(DEMO_MODE_KEY) === '1';
  } catch {
    return false;
  }
};

export const saveDemoProfile = (profile: Record<string, unknown> | null | undefined): void => {
  if (!profile) return;
  writeJson(DEMO_PROFILE_KEY, profile);
  const seed = normalizeDemoSeed(profile.username);
  if (seed) {
    writeJson(DEMO_IDENTITY_KEY, { demo_id: seed });
  }
};

export const ensureDemoProfile = (): DemoProfile => {
  const cached = readJson<DemoProfile | null>(DEMO_PROFILE_KEY, null);
  if (cached && cached.id && cached.username) {
    return cached;
  }
  const identity = ensureDemoIdentity();
  const seed = normalizeDemoSeed(identity.demo_id) || Math.random().toString(36).slice(2, 8);
  const profile: DemoProfile = {
    id: `demo_${seed}`,
    username: `demo_${seed}`,
    access_level: 'A',
    unit_id: null,
    unit: null,
    is_demo: true,
    daily_quota: 10000,
    daily_quota_used: 0,
    daily_quota_date: new Date().toISOString().slice(0, 10),
    created_at: new Date().toISOString()
  };
  saveDemoProfile(profile);
  return profile;
};

export const loadDemoChatState = (): unknown => readJson(DEMO_CHAT_KEY, null);
export const saveDemoChatState = (state: unknown): void => writeJson(DEMO_CHAT_KEY, state);

export const loadDemoWorkspaceState = (): unknown => readJson(DEMO_WORKSPACE_KEY, null);
export const saveDemoWorkspaceState = (state: unknown): void => writeJson(DEMO_WORKSPACE_KEY, state);
