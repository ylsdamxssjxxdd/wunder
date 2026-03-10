export const THEME_MODES = ['dark', 'light'] as const;
export const THEME_PALETTES = ['hula-green', 'eva-orange', 'claw-orange', 'minimal', 'tech-blue'] as const;

export type ThemeMode = (typeof THEME_MODES)[number];
export type ThemePalette = (typeof THEME_PALETTES)[number];

export const DEFAULT_THEME_MODE: ThemeMode = 'light';
export const DEFAULT_THEME_PALETTE: ThemePalette = 'eva-orange';
export const TECH_BLUE_THEME_PALETTE: ThemePalette = 'tech-blue';

export const normalizeThemeMode = (value: unknown): ThemeMode => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  return text === 'dark' ? 'dark' : DEFAULT_THEME_MODE;
};

export const normalizeThemePalette = (value: unknown): ThemePalette => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if ((THEME_PALETTES as readonly string[]).includes(text)) {
    return text as ThemePalette;
  }
  return DEFAULT_THEME_PALETTE;
};

export const isAutoDarkThemePalette = (value: unknown): boolean =>
  normalizeThemePalette(value) === TECH_BLUE_THEME_PALETTE;

export const resolveThemeModeForPalette = (mode: unknown, palette: unknown): ThemeMode =>
  isAutoDarkThemePalette(palette) ? 'dark' : normalizeThemeMode(mode);
