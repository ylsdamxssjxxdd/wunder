/** Join offset-addressed windows only when they extend a contiguous prefix.
 * A durable preview is bounded; it must never replace the accumulated answer.
 * Offsets are UTF-16 units, matching JavaScript string indices. */
export interface ThreadStreamWindow {
  content: string;
  content_offset?: number;
  tail?: string;
  tail_offset?: number;
  live_content?: string;
  live_offset?: number;
}

export const LIVE_STREAM_MAX_CHARS = 262144;

export function extendThreadStreamPrefix(prefix: string, next: ThreadStreamWindow): string {
  const preview = String(next.content || '');
  let text = preview.length > prefix.length ? preview : prefix;
  const tail = String(next.tail || '');
  const end = Number(next.tail_offset ?? next.content_offset ?? preview.length);
  const start = end - tail.length;
  if (tail && Number.isSafeInteger(end) && start >= 0 && start <= text.length && end > text.length) {
    text += tail.slice(text.length - start);
  }
  // Freeze the retained prefix at the cap; rolling it would erase earlier lines
  // and make the last line appear to refresh in place. Settlement reads the body.
  if (text.length <= LIVE_STREAM_MAX_CHARS) return text;
  const endUnit = text.charCodeAt(LIVE_STREAM_MAX_CHARS - 1);
  return text.slice(0, endUnit >= 0xd800 && endUnit <= 0xdbff ? LIVE_STREAM_MAX_CHARS - 1 : LIVE_STREAM_MAX_CHARS);
}

export function reconcileThreadStream(previous: ThreadStreamWindow | undefined, next: ThreadStreamWindow) {
  const prefix = previous?.live_content || previous?.content || '';
  const text = extendThreadStreamPrefix(prefix, next);
  return { live_content: text, live_offset: text.length };
}
