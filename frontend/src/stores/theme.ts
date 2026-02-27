import { defineStore } from 'pinia';

// 用户亮/暗主题持久化键
const THEME_MODE_STORAGE_KEY = 'wille-user-theme';
// 用户配色主题持久化键
const THEME_PALETTE_STORAGE_KEY = 'wille-user-accent-theme';
// 支持的亮/暗主题值，避免非法值污染配置
const THEME_MODES = ['dark', 'light'];
// 支持的配色主题值：Hula 绿 / EVA 橙 / 极简
const THEME_PALETTES = ['hula-green', 'eva-orange', 'minimal'];

// 统一校验并归一化主题值，非法值回落为浅色
const normalizeThemeMode = (value) => {
  if (THEME_MODES.includes(value)) {
    return value;
  }
  return 'light';
};

// 统一校验并归一化配色值，非法值回落为 EVA 橙
const normalizeThemePalette = (value) => {
  if (THEME_PALETTES.includes(value)) {
    return value;
  }
  return 'eva-orange';
};

// 从本地缓存读取亮/暗主题，保证刷新后保持用户偏好
const readThemeModeFromStorage = () => normalizeThemeMode(localStorage.getItem(THEME_MODE_STORAGE_KEY));

// 从本地缓存读取配色主题，保证刷新后保持用户偏好
const readThemePaletteFromStorage = () =>
  normalizeThemePalette(localStorage.getItem(THEME_PALETTE_STORAGE_KEY));

// 同步主题到根节点，供全局样式按 data-user-theme / data-user-accent 渲染
const applyThemeToDocument = (mode, palette) => {
  if (typeof document === 'undefined') return;
  document.documentElement.setAttribute('data-user-theme', mode);
  document.documentElement.setAttribute('data-user-accent', palette);
};

export const useThemeStore = defineStore('theme', {
  state: () => {
    const mode = readThemeModeFromStorage();
    const palette = readThemePaletteFromStorage();
    // 初始化时立即写入 DOM，避免首屏闪烁
    applyThemeToDocument(mode, palette);
    return {
      mode,
      palette
    };
  },
  actions: {
    // 设置亮/暗主题并持久化到本地缓存
    setMode(mode) {
      const nextMode = normalizeThemeMode(mode);
      this.mode = nextMode;
      localStorage.setItem(THEME_MODE_STORAGE_KEY, nextMode);
      applyThemeToDocument(nextMode, this.palette);
    },
    // 设置配色主题并持久化到本地缓存
    setPalette(palette) {
      const nextPalette = normalizeThemePalette(palette);
      this.palette = nextPalette;
      localStorage.setItem(THEME_PALETTE_STORAGE_KEY, nextPalette);
      applyThemeToDocument(this.mode, nextPalette);
    },
    // 快速切换暗色/浅色主题
    toggleMode() {
      const nextMode = this.mode === 'dark' ? 'light' : 'dark';
      this.setMode(nextMode);
    }
  }
});
