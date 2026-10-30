// AI生成
/**
 * 时间戳归一与展示格式化（侧栏悬停提示等）。
 *
 * 会话记录里的时间可能来自三处：运行时的 Unix 秒（数字）、本地 SQLite 迁移
 * 出来的毫秒（数字）以及接口返回的 ISO 文本。展示层只关心「这一刻是什么时候」，
 * 所以这里统一归一成毫秒再按本地时区格式化，避免各处各写一套正则。
 */

/** 归一成 epoch 毫秒；无法识别时返回 0。 */
export function toEpochMs(value: unknown): number {
  if (typeof value === 'number') {
    if (!Number.isFinite(value) || value <= 0) return 0;
    // 10 位是秒，13 位是毫秒；上限判断足以区分，且不会误伤老数据。
    return value < 100_000_000_000 ? value * 1000 : value;
  }
  const text = String(value ?? '').trim();
  if (!text) return 0;
  if (/^\d+(\.\d+)?$/.test(text)) {
    return toEpochMs(Number(text));
  }
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
}

/** 本地时区的 `YYYY-MM-DD HH:mm`；没有可用时间时返回空串。 */
export function formatDateTime(value: unknown): string {
  const ms = toEpochMs(value);
  if (!ms) return '';
  const date = new Date(ms);
  const pad = (part: number) => String(part).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
