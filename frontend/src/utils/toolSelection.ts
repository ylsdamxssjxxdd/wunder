// 共享工具勾选状态使用本地缓存，按 user_id 隔离。
const SHARED_TOOL_SELECTION_STORAGE_PREFIX = 'wille_shared_tool_selection:';

const buildStorageKey = (userId) => {
  const normalized = String(userId || '').trim();
  return `${SHARED_TOOL_SELECTION_STORAGE_PREFIX}${normalized || 'anonymous'}`;
};

// 读取共享工具选择缓存，返回 Set 便于快速判断。
export const loadSharedToolSelection = (userId) => {
  try {
    const raw = localStorage.getItem(buildStorageKey(userId));
    if (!raw) {
      return new Set();
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
    localStorage.setItem(buildStorageKey(userId), JSON.stringify(payload));
  } catch (error) {
    // 忽略本地存储不可用的情况
  }
};
