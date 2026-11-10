export const CHAT_THREAD_DRAG_TYPE = 'application/x-wunder-thread';
export const CHAT_SPLIT_MAX_PANES = 4;

export type ChatSplitCount = 1 | 2 | 4;

export const normalizeChatSplitCount = (value: unknown): ChatSplitCount => {
  const count = Number(value);
  return count === 2 || count === 4 ? count : 1;
};

export const createChatSplitSlots = (count: ChatSplitCount, activeId: string): string[] => {
  if (count === 1) return [];
  return [String(activeId || ''), ...Array.from({ length: count - 1 }, () => '')];
};

export const assignChatSplitSlot = (
  slots: readonly string[],
  slotIndex: number,
  sessionId: string,
  activeId = ''
): string[] => {
  const id = String(sessionId || '').trim();
  if (!id || slotIndex < 0 || slotIndex >= CHAT_SPLIT_MAX_PANES) return [...slots];
  const next = slots.length ? [...slots] : createChatSplitSlots(2, activeId);
  while (next.length < Math.max(2, slotIndex + 1)) next.push('');
  const previousIndex = next.indexOf(id);
  if (previousIndex >= 0 && previousIndex !== slotIndex && next[slotIndex]) {
    next[previousIndex] = next[slotIndex];
    next[slotIndex] = id;
  } else if (previousIndex < 0) {
    next[slotIndex] = id;
  }
  return next;
};

export const closeChatSplitSlot = (slots: readonly string[], slotIndex: number): string[] => {
  if (slotIndex < 0 || slotIndex >= slots.length) return [...slots];
  const retained = slots.filter((id, index) => index !== slotIndex && Boolean(id));
  if (retained.length <= 1) return [];
  const count = retained.length === 2 ? 2 : 4;
  return [...retained, ...Array.from({ length: count - retained.length }, () => '')];
};

export const clampChatSplitRatio = (value: unknown): number => {
  const ratio = Number(value);
  if (!Number.isFinite(ratio)) return 0.5;
  return Math.min(0.75, Math.max(0.25, ratio));
};

export const resolveChatSplitPreviewCount = (
  count: ChatSplitCount,
  slots: readonly string[],
  dragging: boolean
): ChatSplitCount => (count === 2 && dragging && slots.filter(Boolean).length >= 2 ? 4 : count);

export const splitSlotAtPoint = (
  count: ChatSplitCount,
  xRatio: number,
  yRatio: number,
  columnRatio = 0.5,
  rowRatio = 0.5
): number => {
  if (count === 1) return 0;
  if (count === 2) return xRatio < columnRatio ? 0 : 1;
  const row = yRatio < rowRatio ? 0 : 2;
  return row + (xRatio < columnRatio ? 0 : 1);
};
