import { defineStore } from 'pinia';
import {
  DEFAULT_THEME_MODE,
  TECH_BLUE_THEME_PALETTE,
  normalizeThemeMode,
  normalizeThemePalette,
  resolveThemeModeForPalette,
  type ThemeMode,
  type ThemePalette
} from '@/utils/themeAppearance';

const THEME_MODE_STORAGE_KEY = 'wille-user-theme';
const THEME_PALETTE_STORAGE_KEY = 'wille-user-accent-theme';

const readThemeModeFromStorage = () => {
  const raw = localStorage.getItem(THEME_MODE_STORAGE_KEY);
  const normalized = normalizeThemeMode(raw);
  if (raw !== normalized) {
    localStorage.setItem(THEME_MODE_STORAGE_KEY, normalized);
  }
  return normalized;
};

const readThemePaletteFromStorage = () => {
  const raw = localStorage.getItem(THEME_PALETTE_STORAGE_KEY);
  const normalized = normalizeThemePalette(raw);
  if (raw !== normalized) {
    localStorage.setItem(THEME_PALETTE_STORAGE_KEY, normalized);
  }
  return normalized;
};

const applyThemeToDocument = (mode: ThemeMode, palette: ThemePalette) => {
  if (typeof document === 'undefined') return;
  document.documentElement.setAttribute('data-user-theme', mode);
  document.documentElement.setAttribute('data-user-accent', palette);
};

export const useThemeStore = defineStore('theme', {
  state: () => {
    const palette = readThemePaletteFromStorage();
    const storedMode = readThemeModeFromStorage();
    const mode = resolveThemeModeForPalette(storedMode, palette);
    if (mode !== storedMode) {
      localStorage.setItem(THEME_MODE_STORAGE_KEY, mode);
    }
    applyThemeToDocument(mode, palette);
    return {
      mode,
      palette
    };
  },
  actions: {
    setMode(mode: unknown) {
      const nextMode = normalizeThemeMode(mode);
      this.mode = nextMode;
      localStorage.setItem(THEME_MODE_STORAGE_KEY, nextMode);
      applyThemeToDocument(nextMode, this.palette);
    },
    setPalette(palette: unknown) {
      const nextPalette = normalizeThemePalette(palette);
      const nextMode =
        nextPalette === TECH_BLUE_THEME_PALETTE
          ? 'dark'
          : this.palette === TECH_BLUE_THEME_PALETTE && this.mode === 'dark'
            ? DEFAULT_THEME_MODE
            : this.mode;

      this.palette = nextPalette;
      this.mode = nextMode;
      localStorage.setItem(THEME_PALETTE_STORAGE_KEY, nextPalette);
      localStorage.setItem(THEME_MODE_STORAGE_KEY, nextMode);
      applyThemeToDocument(nextMode, nextPalette);
    },
    toggleMode() {
      const nextMode = this.mode === 'dark' ? 'light' : 'dark';
      this.setMode(nextMode);
    }
  }
});
