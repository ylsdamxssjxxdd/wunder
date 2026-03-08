// 共享工具勾选状态使用本地缓存，按 user_id 隔离。
const SHARED_TOOL_SELECTION_STORAGE_PREFIX = 'beeroom_shared_tool_selection:';
const LEGACY_SHARED_TOOL_SELECTION_STORAGE_PREFIX = 'wille_shared_tool_selection:';

const buildStorageKey = (prefix, userId) => {
  const normalized = String(userId || '').trim();
  return `${prefix}${normalized || 'anonymous'}`;
};

// 读取共享工具选择缓存，返回 Set 便于快速判断。
export const loadSharedToolSelection = (userId) => {
  try {
    const primaryKey = buildStorageKey(SHARED_TOOL_SELECTION_STORAGE_PREFIX, userId);
    const legacyKey = buildStorageKey(LEGACY_SHARED_TOOL_SELECTION_STORAGE_PREFIX, userId);
    const raw = localStorage.getItem(primaryKey) ?? localStorage.getItem(legacyKey);
    if (!raw) {
      return new Set();
    }
    if (!localStorage.getItem(primaryKey)) {
      localStorage.setItem(primaryKey, raw);
    }
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      return new Set(parsed.map((name) => String(name)));
    }
    return new Set();
  } catch (error) {
    return new Set();
  }
};

// 保存共享工具勾选列表。
export const saveSharedToolSelection = (userId, selectedSet) => {
  try {
    const payload = Array.from(selectedSet || []).map((name) => String(name));
    const serialized = JSON.stringify(payload);
    localStorage.setItem(buildStorageKey(SHARED_TOOL_SELECTION_STORAGE_PREFIX, userId), serialized);
    localStorage.setItem(buildStorageKey(LEGACY_SHARED_TOOL_SELECTION_STORAGE_PREFIX, userId), serialized);
  } catch (error) {
    // 忽略本地存储不可用的情况
  }
};
