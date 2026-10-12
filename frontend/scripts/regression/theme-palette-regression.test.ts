import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import {
  DEFAULT_THEME_PALETTE,
  SELECTABLE_THEME_PALETTES,
  THEME_PALETTE_LABEL_KEYS,
  THEME_PALETTE_SWATCHES,
  normalizeThemePalette,
  type ThemePalette,
  type ThemeSwatchPalette
} from '../../src/utils/themeAppearance';

const baseCss = readFileSync(resolve(process.cwd(), 'src/styles/base.css'), 'utf8');

/** The `:root[data-user-accent='id'] { … }` body, or null when absent. */
const paletteBlock = (id: string): string | null => {
  const start = baseCss.indexOf(`:root[data-user-accent='${id}']`);
  if (start < 0) return null;
  const open = baseCss.indexOf('{', start);
  const close = baseCss.indexOf('}', open);
  return baseCss.slice(open + 1, close);
};

const hexToRgb = (hex: string): string => {
  const value = hex.replace('#', '');
  return [0, 2, 4]
    .map((at) => Number.parseInt(value.slice(at, at + 2), 16))
    .join(', ');
};

test('the picker offers four distinct palettes and one of them is the default', () => {
  assert.equal(SELECTABLE_THEME_PALETTES.length, 4);
  assert.equal(new Set<string>(SELECTABLE_THEME_PALETTES).size, 4);
  assert.ok(
    (SELECTABLE_THEME_PALETTES as readonly ThemePalette[]).includes(DEFAULT_THEME_PALETTE),
    'a fresh install must land on a tile the picker can show as active'
  );
  for (const palette of SELECTABLE_THEME_PALETTES) {
    assert.equal(normalizeThemePalette(palette), palette);
  }
});

test('every selectable palette ships a label key and a swatch', () => {
  for (const palette of SELECTABLE_THEME_PALETTES) {
    const key = THEME_PALETTE_LABEL_KEYS[palette];
    assert.match(String(key), /^messenger\.[A-Za-z0-9.]+$/, `${palette} needs an i18n key`);
    const swatch = THEME_PALETTE_SWATCHES[palette as ThemeSwatchPalette];
    assert.match(swatch.accent, /^#[0-9a-f]{6}$/, `${palette} accent`);
    assert.match(swatch.tint, /^#[0-9a-f]{6}$/, `${palette} tint`);
  }
});

test('the tile swatches match the palette definitions in base.css', () => {
  for (const palette of SELECTABLE_THEME_PALETTES) {
    const block = paletteBlock(palette);
    assert.ok(block, `base.css has no :root[data-user-accent='${palette}'] block`);
    const swatch = THEME_PALETTE_SWATCHES[palette as ThemeSwatchPalette];
    // The tile paints --ui-accent and --ui-accent-soft-3; the card grid paints
    // --ui-accent-rgb, so all three must agree with the swatch.
    assert.ok(
      block.includes(`--ui-accent: ${swatch.accent};`),
      `${palette}: tile accent ${swatch.accent} is not --ui-accent in base.css`
    );
    assert.ok(
      block.includes(`--ui-accent-soft-3: ${swatch.tint};`),
      `${palette}: tile tint ${swatch.tint} is not --ui-accent-soft-3 in base.css`
    );
  }
});

test('each palette keeps its rgb triple in step with its accent hex', () => {
  const root = baseCss.slice(0, baseCss.indexOf(':root['));
  const blocks = new Map<string, string | null>([['default', root]]);
  for (const match of baseCss.matchAll(/data-user-accent='([^']+)'\]/g)) {
    blocks.set(match[1], paletteBlock(match[1]));
  }
  for (const [name, block] of blocks) {
    assert.ok(block, `${name}: palette block vanished`);
    const hex = /--ui-accent:\s*(#[0-9a-f]{6});/.exec(block as string)?.[1];
    const rgb = /--ui-accent-rgb:\s*([0-9, ]+);/.exec(block as string)?.[1];
    assert.ok(hex && rgb, `${name}: needs both --ui-accent and --ui-accent-rgb`);
    // The heatmap ramp is rgba(--ui-accent-rgb); a stale triple would paint a
    // different colour than every other accent surface.
    assert.equal(
      (rgb as string).replace(/\s+/g, ' ').trim(),
      hexToRgb(hex as string),
      `${name}: --ui-accent-rgb disagrees with --ui-accent`
    );
  }
});

test('unknown and legacy values still resolve to a real palette', () => {
  assert.equal(normalizeThemePalette(''), DEFAULT_THEME_PALETTE);
  assert.equal(normalizeThemePalette('neon-pink'), DEFAULT_THEME_PALETTE);
  assert.equal(normalizeThemePalette(' Tech-Blue '), 'tech-blue');
  assert.equal(normalizeThemePalette(null), DEFAULT_THEME_PALETTE);
});

test('the server accepts every palette the picker can write', () => {
  // PATCH /auth/me/preferences clamps an unknown `theme_palette` back to the
  // default and the echo overwrites the local copy, so a palette missing from
  // the Rust whitelist snaps the UI back on the next appearance load.
  const authApi = readFileSync(
    resolve(process.cwd(), '..', 'crates', 'wunder-runtime', 'src', 'api', 'auth.rs'),
    'utf8'
  );
  const block = /fn normalize_theme_palette[\s\S]*?\n}/.exec(authApi)?.[0];
  assert.ok(block, 'normalize_theme_palette vanished from the auth API');
  const accepted = new Set([...block.matchAll(/"([a-z-]+)"/g)].map((match) => match[1]));
  for (const palette of SELECTABLE_THEME_PALETTES) {
    assert.ok(accepted.has(palette), `server clamps the selectable palette ${palette}`);
  }
});
