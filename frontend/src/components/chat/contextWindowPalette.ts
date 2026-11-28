// Slot colour palette matched to the desktop `ContextColors`; keep the hex
// values here and do not re-tune from the cloud side.

export type Palette = {
  ink: string;
  fill: string;
  line: string;
  name: string;
  value: string;
  hoverFill: string;
  hoverLine: string;
};

const PART_PALETTE: Record<number, Palette> = {
  1: {
    ink: '#8B5CF6',
    fill: '#F2EDFE',
    line: '#E3D8FD',
    name: '#6D4CBD',
    value: '#8A7AC0',
    hoverFill: '#E9E0FD',
    hoverLine: '#BBA6F6'
  },
  2: {
    ink: '#0EA5E9',
    fill: '#E5F5FD',
    line: '#C5E9FA',
    name: '#1381B3',
    value: '#5699BB',
    hoverFill: '#D1EEFB',
    hoverLine: '#82D0F4'
  },
  3: {
    ink: '#10B981',
    fill: '#E5F7F1',
    line: '#C5E8D7',
    name: '#148F68',
    value: '#57A18F',
    hoverFill: '#D2F2E7',
    hoverLine: '#67C9A6'
  },
  4: {
    ink: '#EA580C',
    fill: '#FDEDE4',
    line: '#F6D7C5',
    name: '#B14914',
    value: '#B2795E',
    hoverFill: '#FBDDCC',
    hoverLine: '#F2A275'
  },
  5: {
    ink: '#4F46E5',
    fill: '#ECEBFC',
    line: '#D5D3F9',
    name: '#423CB0',
    value: '#7171B9',
    hoverFill: '#DEDCFA',
    hoverLine: '#918CF1'
  }
};

const UNKNOWN_PALETTE: Palette = {
  ink: '#A8AFB8',
  fill: '#F4F5F7',
  line: '#E2E5E9',
  name: '#6B7280',
  value: '#9AA0A8',
  hoverFill: '#EEF0F3',
  hoverLine: '#D7DBE0'
};

export const paletteFor = (slot: number): Palette => PART_PALETTE[slot] ?? UNKNOWN_PALETTE;
