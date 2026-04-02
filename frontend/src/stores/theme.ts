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

const applyThemeToDocument = (palette: ThemePalette) => {
  if (typeof document === 'undefined') return;
  document.documentElement.setAttribute('data-user-accent', palette);
  // Remove legacy data-user-theme attribute
  document.documentElement.removeAttribute('data-user-theme');
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
