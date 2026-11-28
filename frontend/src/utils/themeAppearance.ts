// AI生成
/**
 * 强调色板 id。`data-user-accent` 与本地/服务端偏好都存这个值。
 *
 * 选择器只暴露 SELECTABLE_THEME_PALETTES 四套；其余是历史值，仍能被
 * normalizeThemePalette 接受，保证老用户的偏好不会掉回默认色。
 * forest 使用薄荷绿强调色；mountain 是浅灰底主题；deep-sea 是暗色主题。
 */
export const THEME_PALETTES = [
  'eva-orange',
  'forest',
  'mountain',
  'deep-sea',
  'claw-orange',
  'minimal',
  'tech-blue'
] as const;

export type ThemePalette = (typeof THEME_PALETTES)[number];

export const DEFAULT_THEME_PALETTE: ThemePalette = 'eva-orange';

/** 设置页「主题模式」卡片提供的四套，顺序即展示顺序。 */
export const SELECTABLE_THEME_PALETTES = [
  'forest',
  'mountain',
  'deep-sea',
  'eva-orange'
] as const satisfies readonly ThemePalette[];

/** 每张卡片对应的 i18n 名称键；历史色板没有卡片，不需要名称。 */
export const THEME_PALETTE_LABEL_KEYS: Partial<Record<ThemePalette, string>> = {
  forest: 'messenger.settingsPage.general.themeForest',
  mountain: 'messenger.settingsPage.general.themeMountain',
  'deep-sea': 'messenger.settingsPage.general.themeDeepSea',
  'eva-orange': 'messenger.settingsPage.general.themeParchment'
};

/** 预览卡取的两档色值，与 base.css 里同名 `data-user-accent` 块的
 *  `--ui-accent` / `--ui-accent-soft` 一致；改色板时两处一起改。
 *  橙汁（eva-orange）的色值以桌面版 crates/wunder-desktop/frontend/ui/design_tokens.slint 的
 *  terracotta 为准：accent #F58A24、soft #FDF0DE。 */
export type ThemeSwatchPalette = (typeof SELECTABLE_THEME_PALETTES)[number];

export const THEME_PALETTE_SWATCHES: Record<ThemeSwatchPalette, { accent: string; tint: string }> = {
  forest: { accent: '#16ad8d', tint: '#e7f7f2' },
  mountain: { accent: '#7c878c', tint: '#eef1f2' },
  'deep-sea': { accent: '#5fa8cf', tint: '#122b3b' },
  'eva-orange': { accent: '#f58a24', tint: '#fef7ee' }
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
