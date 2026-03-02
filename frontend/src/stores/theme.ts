import { defineStore } from 'pinia';

const THEME_MODE_STORAGE_KEY = 'wille-user-theme';
const THEME_PALETTE_STORAGE_KEY = 'wille-user-accent-theme';

const DEFAULT_THEME_MODE = 'light';
const DEFAULT_THEME_PALETTE = 'eva-orange';

const THEME_MODES = ['dark', 'light'] as const;
const THEME_PALETTES = ['hula-green', 'eva-orange', 'minimal'] as const;

type ThemeMode = (typeof THEME_MODES)[number];
type ThemePalette = (typeof THEME_PALETTES)[number];

const normalizeThemeMode = (value: unknown) => {
  if (typeof value === 'string' && (THEME_MODES as readonly string[]).includes(value)) {
    return value as ThemeMode;
  }
  return DEFAULT_THEME_MODE as ThemeMode;
};

const normalizeThemePalette = (value: unknown) => {
  if (typeof value === 'string' && (THEME_PALETTES as readonly string[]).includes(value)) {
    return value as ThemePalette;
  }
  return DEFAULT_THEME_PALETTE as ThemePalette;
};

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
    const mode = readThemeModeFromStorage();
    const palette = readThemePaletteFromStorage();
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
      this.palette = nextPalette;
      localStorage.setItem(THEME_PALETTE_STORAGE_KEY, nextPalette);
      applyThemeToDocument(this.mode, nextPalette);
    },
    toggleMode() {
      const nextMode = this.mode === 'dark' ? 'light' : 'dark';
      this.setMode(nextMode);
    }
  }
});
