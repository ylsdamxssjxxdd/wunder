export type MessageVirtualWindowItem = {
  key: string;
};

export type MessageVirtualWindowOptions<T extends MessageVirtualWindowItem> = {
  items: T[];
  enabled: boolean;
  scrollTop: number;
  viewportHeight: number;
  overscan: number;
  tailPinCount: number;
  estimatedHeight: number;
  resolveHeight: (key: string) => number;
  layoutVersion?: number;
};

export type MessageVirtualWindowResult<T extends MessageVirtualWindowItem> = {
  enabled: boolean;
  topPadding: number;
  bottomPadding: number;
  startIndex: number;
  endIndex: number;
  tailStartIndex: number;
  totalHeight: number;
  visibleItems: T[];
  tailItems: T[];
};

const clamp = (value: number, min: number, max: number): number => Math.max(min, Math.min(max, value));

const normalizeSize = (value: number, fallback: number): number =>
  Number.isFinite(value) && value > 0 ? Math.floor(value) : fallback;

const normalizeHeight = (value: number, fallback: number): number =>
  Number.isFinite(value) && value > 0 ? Math.floor(value) : fallback;

// --- Row-segment height estimation -------------------------------------
//
// A virtual row is estimated from its key alone. Until a row reports its real
// height, the estimate is grouped by row segment: every `turn:` / `tturn:` key is
// one composite user+assistant round, `conversation-greeting` is the greeting
// bubble, `world:` keys belong to the world conversation, everything else is a
// plain message row. Rows sharing a segment also share a running average of the
// heights that were actually measured for that segment, so the estimator learns
// the real per-segment height instead of the flat `turn x 2` rule.

export type MessageVirtualRowSegment = 'greeting' | 'turn' | 'world' | 'message';

export const MESSAGE_VIRTUAL_ROW_SEGMENTS: readonly MessageVirtualRowSegment[] = Object.freeze([
  'greeting',
  'turn',
  'world',
  'message'
]);

// Default multiplier applied to `baseHeight` for a segment before any sample is
// measured. A turn row renders the user bubble plus the assistant bubble, hence
// the factor of two; the other segments render a single bubble.
export const MESSAGE_VIRTUAL_SEGMENT_MULTIPLIERS: Record<MessageVirtualRowSegment, number> = Object.freeze({
  greeting: 1,
  turn: 2,
  world: 1,
  message: 1
});

export const resolveMessageVirtualRowSegment = (key: string): MessageVirtualRowSegment => {
  const normalized = String(key || '').trim();
  if (normalized === 'conversation-greeting' || normalized.startsWith('greeting:')) {
    return 'greeting';
  }
  if (normalized.startsWith('turn:') || normalized.startsWith('tturn:')) {
    return 'turn';
  }
  if (normalized.startsWith('world:') || normalized.startsWith('world-')) {
    return 'world';
  }
  return 'message';
};

export type MessageVirtualHeightEstimatorOptions = {
  baseHeight: number;
  fallbackHeight?: number;
  segmentMultipliers?: Partial<Record<MessageVirtualRowSegment, number>>;
};

export type MessageVirtualHeightEstimator = {
  /** Measured height for the key, else the learned segment average, else the segment default. */
  resolve: (key: string) => number;
  /** Store a measured height. Returns true when the stored value changed. */
  record: (key: string, height: number) => boolean;
  measuredHeight: (key: string) => number;
  segmentEstimate: (segment: MessageVirtualRowSegment) => number;
  /** Measured key/height pairs, in insertion order (for persistence). */
  snapshot: () => Array<[string, number]>;
  hydrate: (entries: Iterable<[string, number]>) => void;
  clear: () => void;
  size: () => number;
};

export const createMessageVirtualHeightEstimator = (
  options: MessageVirtualHeightEstimatorOptions
): MessageVirtualHeightEstimator => {
  const baseHeight = normalizeHeight(options.baseHeight, 96);
  const fallbackHeight = normalizeHeight(options.fallbackHeight ?? baseHeight, baseHeight);
  const multipliers: Record<MessageVirtualRowSegment, number> = {
    ...MESSAGE_VIRTUAL_SEGMENT_MULTIPLIERS,
    ...(options.segmentMultipliers || {})
  };
  const measured = new Map<string, number>();
  const segmentTotal = new Map<MessageVirtualRowSegment, number>();
  const segmentSamples = new Map<MessageVirtualRowSegment, number>();

  const segmentDefault = (segment: MessageVirtualRowSegment): number =>
    normalizeHeight(Math.floor(baseHeight * (multipliers[segment] || 1)), fallbackHeight);

  const segmentEstimate = (segment: MessageVirtualRowSegment): number => {
    const samples = segmentSamples.get(segment) || 0;
    if (samples > 0) {
      return Math.max(1, Math.floor((segmentTotal.get(segment) || 0) / samples));
    }
    return segmentDefault(segment);
  };

  const resolve = (key: string): number => {
    const normalized = String(key || '').trim();
    const cached = measured.get(normalized);
    if (cached !== undefined && Number.isFinite(cached) && cached > 0) {
      return cached;
    }
    return segmentEstimate(resolveMessageVirtualRowSegment(normalized));
  };

  const record = (key: string, height: number): boolean => {
    const normalized = String(key || '').trim();
    const value = Math.floor(Number(height) || 0);
    if (!normalized || value <= 0) {
      return false;
    }
    const previous = measured.get(normalized);
    if (previous === value) {
      return false;
    }
    const segment = resolveMessageVirtualRowSegment(normalized);
    measured.set(normalized, value);
    segmentTotal.set(segment, (segmentTotal.get(segment) || 0) - (previous || 0) + value);
    segmentSamples.set(segment, (segmentSamples.get(segment) || 0) + (previous === undefined ? 1 : 0));
    return true;
  };

  const hydrate = (entries: Iterable<[string, number]>): void => {
    for (const entry of entries) {
      if (!entry) continue;
      const [key, height] = entry;
      record(String(key || ''), Number(height));
    }
  };

  const clear = (): void => {
    measured.clear();
    segmentTotal.clear();
    segmentSamples.clear();
  };

  return {
    resolve,
    record,
    measuredHeight: (key: string) => measured.get(String(key || '').trim()) || 0,
    segmentEstimate,
    snapshot: () => Array.from(measured.entries()),
    hydrate,
    clear,
    size: () => measured.size
  };
};

// --- Prefix-sum offset lookup ------------------------------------------
//
// `resolveVirtualOffsetTop` used to add `resolveHeight(...)` once per row from 0
// to `index`, so its cost grew linearly with the row count. It now reads a
// prefix-sum array: the running total is built once per key list and every later
// lookup is an O(1) array read (zero `resolveHeight` calls).

type VirtualOffsetPrefixCacheEntry = {
  resolveHeight: (key: string) => number;
  prefix: number[];
};

const virtualOffsetPrefixCache = new WeakMap<object, VirtualOffsetPrefixCacheEntry>();

export const buildVirtualOffsetPrefix = (
  keys: string[],
  resolveHeight: (key: string) => number,
  fallbackHeight = 0
): number[] => {
  const list = Array.isArray(keys) ? keys : [];
  const prefix = new Array<number>(list.length + 1);
  prefix[0] = 0;
  for (let index = 0; index < list.length; index += 1) {
    const value = normalizeHeight(resolveHeight(list[index] || ''), fallbackHeight);
    prefix[index + 1] = prefix[index] + value;
  }
  return prefix;
};

export const resolveVirtualOffsetTopFromPrefix = (prefix: number[], index: number): number => {
  if (!Array.isArray(prefix) || prefix.length === 0) {
    return 0;
  }
  const safeIndex = clamp(Math.trunc(index), 0, prefix.length - 1);
  return prefix[safeIndex] || 0;
};

export const resolveVirtualOffsetTop = (
  keys: string[],
  index: number,
  resolveHeight: (key: string) => number,
  prefix?: number[]
): number => {
  const list = Array.isArray(keys) ? keys : [];
  const safeIndex = clamp(Math.trunc(index), 0, Math.max(0, list.length));
  if (safeIndex <= 0) {
    return 0;
  }
  if (Array.isArray(prefix) && prefix.length >= safeIndex + 1) {
    return prefix[safeIndex] || 0;
  }
  const cached = virtualOffsetPrefixCache.get(list);
  if (cached && cached.resolveHeight === resolveHeight && cached.prefix.length === list.length + 1) {
    return cached.prefix[safeIndex] || 0;
  }
  const built = buildVirtualOffsetPrefix(list, resolveHeight);
  virtualOffsetPrefixCache.set(list, { resolveHeight, prefix: built });
  return built[safeIndex] || 0;
};

const heightPrefixCache = new WeakMap<object, {
  version: number; length: number; fallback: number;
  resolveHeight: (key: string) => number; prefix: number[];
}>();

const buildHeightPrefix = <T extends MessageVirtualWindowItem>(
  items: T[],
  resolveHeight: (key: string) => number,
  fallbackHeight: number
): number[] => {
  const prefix = new Array(items.length + 1);
  prefix[0] = 0;
  for (let index = 0; index < items.length; index += 1) {
    prefix[index + 1] =
      prefix[index] + normalizeHeight(resolveHeight(items[index]?.key || ''), fallbackHeight);
  }
  return prefix;
};

const findFirstItemEndingAfter = (
  prefix: number[],
  endExclusive: number,
  offset: number
): number => {
  let low = 0;
  let high = Math.max(0, endExclusive);
  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    if ((prefix[mid + 1] || 0) >= offset) {
      high = mid;
    } else {
      low = mid + 1;
    }
  }
  return low;
};

const findFirstPrefixAtLeast = (
  prefix: number[],
  start: number,
  endExclusive: number,
  offset: number
): number => {
  let low = Math.max(0, start);
  let high = Math.max(low, endExclusive);
  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    if ((prefix[mid] || 0) >= offset) {
      high = mid;
    } else {
      low = mid + 1;
    }
  }
  return low;
};

export const buildMessageVirtualWindow = <T extends MessageVirtualWindowItem>(
  options: MessageVirtualWindowOptions<T>
): MessageVirtualWindowResult<T> => {
  const items = Array.isArray(options.items) ? options.items : [];
  if (!items.length) {
    return {
      enabled: false,
      topPadding: 0,
      bottomPadding: 0,
      startIndex: 0,
      endIndex: 0,
      tailStartIndex: 0,
      totalHeight: 0,
      visibleItems: [],
      tailItems: []
    };
  }

  if (!options.enabled) {
    return {
      enabled: false,
      topPadding: 0,
      bottomPadding: 0,
      startIndex: 0,
      endIndex: items.length,
      tailStartIndex: items.length,
      totalHeight: 0,
      visibleItems: items,
      tailItems: []
    };
  }

  const estimatedHeight = normalizeSize(options.estimatedHeight, 96);
  const viewportHeight = normalizeSize(options.viewportHeight, estimatedHeight * 8);
  const overscan = clamp(normalizeSize(options.overscan, 6), 0, 64);
  const tailPinCount = clamp(normalizeSize(options.tailPinCount, 8), 0, items.length);
  const tailStart = Math.max(0, items.length - tailPinCount);
  const cached = heightPrefixCache.get(items);
  const version = options.layoutVersion;
  const heightPrefix = version !== undefined && cached?.version === version &&
    cached.length === items.length && cached.fallback === estimatedHeight &&
    cached.resolveHeight === options.resolveHeight
    ? cached.prefix
    : buildHeightPrefix(items, options.resolveHeight, estimatedHeight);
  if (version !== undefined && heightPrefix !== cached?.prefix) {
    heightPrefixCache.set(items, { version, length: items.length, fallback: estimatedHeight,
      resolveHeight: options.resolveHeight, prefix: heightPrefix });
  }
  const totalHeight = heightPrefix[items.length] || 0;
  if (tailStart <= 0) {
    return {
      enabled: true,
      topPadding: 0,
      bottomPadding: 0,
      startIndex: 0,
      endIndex: 0,
      tailStartIndex: 0,
      totalHeight,
      visibleItems: [],
      tailItems: items
    };
  }

  const clampedScrollTop = Math.max(0, Math.floor(options.scrollTop || 0));
  let start = findFirstItemEndingAfter(heightPrefix, tailStart, clampedScrollTop);
  start = Math.max(0, start - overscan);
  const topPadding = heightPrefix[start] || 0;

  const viewportBottom = clampedScrollTop + viewportHeight;
  let end = findFirstPrefixAtLeast(heightPrefix, start, tailStart, viewportBottom);
  end = Math.min(tailStart, end + overscan);

  const headHeight = heightPrefix[tailStart] || 0;
  const bottomPadding = Math.max(0, headHeight - (heightPrefix[end] || 0));

  return {
    enabled: true,
    topPadding,
    bottomPadding,
    startIndex: start,
    endIndex: end,
    tailStartIndex: tailStart,
    totalHeight,
    visibleItems: items.slice(start, end),
    tailItems: items.slice(tailStart)
  };
};
