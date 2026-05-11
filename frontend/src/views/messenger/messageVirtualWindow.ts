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

export const resolveVirtualOffsetTop = (
  keys: string[],
  index: number,
  resolveHeight: (key: string) => number
): number => {
  const safeIndex = clamp(Math.trunc(index), 0, Math.max(0, keys.length));
  let top = 0;
  for (let cursor = 0; cursor < safeIndex; cursor += 1) {
    top += resolveHeight(keys[cursor] || '');
  }
  return top;
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
  const heightPrefix = buildHeightPrefix(items, options.resolveHeight, estimatedHeight);
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
