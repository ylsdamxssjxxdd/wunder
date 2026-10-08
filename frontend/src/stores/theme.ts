import { defineStore } from 'pinia';
import {
  DEFAULT_THEME_PALETTE,
  normalizeThemePalette,
  type ThemePalette
} from '@/utils/themeAppearance';

const THEME_PALETTE_STORAGE_KEY = 'beeroom-user-accent-theme';
const LEGACY_PALETTE_STORAGE_KEY = 'wille-user-accent-theme';

const readPaletteFromStorage = () => {
  const primary = localStorage.getItem(THEME_PALETTE_STORAGE_KEY);
  if (primary !== null) return primary;
  const legacy = localStorage.getItem(LEGACY_PALETTE_STORAGE_KEY);
  if (legacy !== null) {
    localStorage.setItem(THEME_PALETTE_STORAGE_KEY, legacy);
  }
  return legacy;
};

/**
 * 只应用「强调色」。
 *
 * B6 结论：深/浅主题切换（`data-user-theme`）在原型阶段已被移除——没有任何 UI 开关，
 * 而 `tech-blue-dark.css`（164 KB）与 `light.css` 整份都挂在 `data-user-theme` 上，
 * 永远匹配不到元素。因此这里不再保留 `removeAttribute('data-user-theme')` 这种
 * 「删掉一个没人设置的属性」的假动作，样式侧的死规则也一并删除。
 */
const applyThemeToDocument = (palette: ThemePalette) => {
  if (typeof document === 'undefined') return;
  document.documentElement.setAttribute('data-user-accent', palette);
};

export const useThemeStore = defineStore('theme', {
  state: () => {
    const palette = normalizeThemePalette(readPaletteFromStorage());
    applyThemeToDocument(palette);
    return { palette };
  },
  actions: {
    setPalette(palette: unknown) {
      const next = normalizeThemePalette(palette);
      this.palette = next;
      localStorage.setItem(THEME_PALETTE_STORAGE_KEY, next);
      localStorage.setItem(LEGACY_PALETTE_STORAGE_KEY, next);
      applyThemeToDocument(next);
    }
  }
});
