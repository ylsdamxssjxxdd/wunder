const DEMO_MODE_KEY = 'demo_mode';
const DEMO_IDENTITY_KEY = 'demo_identity';
const DEMO_PROFILE_KEY = 'demo_profile';
const DEMO_CHAT_KEY = 'demo_chat_state';
const DEMO_WORKSPACE_KEY = 'demo_workspace_state';
const DEMO_TOKEN_KEY = 'demo_access_token';

// 读取本地 JSON 数据，解析失败则返回兜底值
const readJson = (key, fallback) => {
  if (!key) return fallback;
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return fallback;
    return JSON.parse(raw);
  } catch (error) {
    return fallback;
  }
};

// 写入本地 JSON 数据，避免序列化异常导致页面崩溃
const writeJson = (key, value) => {
  if (!key) return;
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch (error) {
    // 本地存储不可用时忽略
  }
};

const normalizeDemoSeed = (value) => {
  const raw = String(value || '').trim();
  if (!raw) return '';
  const trimmed = raw.startsWith('demo_') ? raw.slice(5) : raw;
  const cleaned = trimmed.replace(/[^a-zA-Z0-9_-]/g, '');
  return cleaned.slice(0, 24);
};

// 生成演示用户随机 ID，用于后端免登录创建账号
export const ensureDemoIdentity = () => {
  const cached = readJson(DEMO_IDENTITY_KEY, null);
  if (cached && cached.demo_id) {
    return cached;
  }
  const cachedProfile = readJson(DEMO_PROFILE_KEY, null);
  const seed =
    normalizeDemoSeed(cachedProfile?.username) ||
    normalizeDemoSeed(cachedProfile?.id) ||
    Math.random().toString(36).slice(2, 10);
  const identity = { demo_id: seed };
  writeJson(DEMO_IDENTITY_KEY, identity);
  return identity;
};

export const getDemoToken = () => {
  try {
    return localStorage.getItem(DEMO_TOKEN_KEY) || '';
  } catch (error) {
    return '';
  }
};

export const setDemoToken = (token) => {
  if (!token) return;
  try {
    localStorage.setItem(DEMO_TOKEN_KEY, token);
  } catch (error) {
    // 本地存储不可用时忽略
  }
};

export const clearDemoToken = () => {
  try {
    localStorage.removeItem(DEMO_TOKEN_KEY);
  } catch (error) {
    // 本地存储不可用时忽略
  }
};

// 演示模式开关
export const enableDemoMode = () => {
  try {
    localStorage.setItem(DEMO_MODE_KEY, '1');
  } catch (error) {
    // 本地存储不可用时忽略
  }
};

export const disableDemoMode = () => {
  try {
    localStorage.removeItem(DEMO_MODE_KEY);
  } catch (error) {
    // 本地存储不可用时忽略
  }
};

export const isDemoMode = () => {
  try {
    return localStorage.getItem(DEMO_MODE_KEY) === '1';
  } catch (error) {
    return false;
  }
};

export const saveDemoProfile = (profile) => {
  if (!profile) return;
  writeJson(DEMO_PROFILE_KEY, profile);
  const seed = normalizeDemoSeed(profile?.username);
  if (seed) {
    writeJson(DEMO_IDENTITY_KEY, { demo_id: seed });
  }
};

// 生成随机的演示用户信息并缓存到本地
export const ensureDemoProfile = () => {
  const cached = readJson(DEMO_PROFILE_KEY, null);
  if (cached && cached.id && cached.username) {
    return cached;
  }
  const identity = ensureDemoIdentity();
  const seed = normalizeDemoSeed(identity?.demo_id) || Math.random().toString(36).slice(2, 8);
  const profile = {
    id: `demo_${seed}`,
    username: `demo_${seed}`,
    access_level: 'A',
    is_demo: true,
    daily_quota: 10000,
    daily_quota_used: 0,
    daily_quota_date: new Date().toISOString().slice(0, 10),
    created_at: new Date().toISOString()
  };
  saveDemoProfile(profile);
  return profile;
};

// 演示模式的聊天数据读写
export const loadDemoChatState = () => readJson(DEMO_CHAT_KEY, null);
export const saveDemoChatState = (state) => writeJson(DEMO_CHAT_KEY, state);

// 演示模式的工作区数据读写
export const loadDemoWorkspaceState = () => readJson(DEMO_WORKSPACE_KEY, null);
export const saveDemoWorkspaceState = (state) => writeJson(DEMO_WORKSPACE_KEY, state);
