export const THEME_PALETTES = ['hula-green', 'eva-orange', 'claw-orange', 'minimal', 'tech-blue'] as const;

export type ThemePalette = (typeof THEME_PALETTES)[number];

export const DEFAULT_THEME_PALETTE: ThemePalette = 'eva-orange';

export const normalizeThemePalette = (value: unknown): ThemePalette => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if ((THEME_PALETTES as readonly string[]).includes(text)) {
    return text as ThemePalette;
  }
  return DEFAULT_THEME_PALETTE;
};
