import { defineStore } from 'pinia';

// 用户主题持久化的本地存储键
const THEME_STORAGE_KEY = 'wille-user-theme';
// 支持的主题值，避免非法值污染配置
const THEME_MODES = ['dark', 'light'];

// 统一校验并归一化主题值，非法值回落为浅色
const normalizeThemeMode = (value) => {
  if (THEME_MODES.includes(value)) {
    return value;
  }
  return 'light';
};

// 从本地缓存读取主题，保证刷新后保持用户偏好
const readThemeFromStorage = () => normalizeThemeMode(localStorage.getItem(THEME_STORAGE_KEY));

// 同步主题到根节点，供全局样式按 data-user-theme 渲染
const applyThemeToDocument = (mode) => {
  if (typeof document === 'undefined') return;
  document.documentElement.setAttribute('data-user-theme', mode);
};

export const useThemeStore = defineStore('theme', {
  state: () => {
    const mode = readThemeFromStorage();
    // 初始化时立即写入 DOM，避免首屏闪烁
    applyThemeToDocument(mode);
    return {
      mode
    };
  },
  actions: {
    // 设置主题并持久化到本地缓存
    setMode(mode) {
      const nextMode = normalizeThemeMode(mode);
      this.mode = nextMode;
      localStorage.setItem(THEME_STORAGE_KEY, nextMode);
      applyThemeToDocument(nextMode);
    },
    // 快速切换暗色/浅色主题
    toggleMode() {
      const nextMode = this.mode === 'dark' ? 'light' : 'dark';
      this.setMode(nextMode);
    }
  }
});
