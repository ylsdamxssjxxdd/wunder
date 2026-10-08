/**
 * 聊天区右侧「用户轮次刻度」的数据投影。
 *
 * 对齐桌面端 `frontend-slint/src/timeline.rs:187-203`：每个用户轮次一个刻度，
 * 值是「该轮次行号 / 总行数」的小数；总行数 ≤1 时不显示；上限 100 个，
 * 超出时只保留**最新** 100 个。
 *
 * 这里刻意只做一次 O(轮次数) 的遍历：调用方把它放进 `computed`，行数/轮次数
 * 不变时不会重算，滚动事件里更不会碰到它。
 */

export type TurnMarkRow = {
  kind?: string;
  rootTurnId?: string;
  key?: string;
};

export type TurnMarkOptions = {
  /**
   * 当前渲染窗口在**完整**行数组里的起点。
   * 虚拟化只渲染一个窗口时，刻度仍是「绝对行号 / 总行数」，滚动时刻度条不会整体平移。
   */
  rowOffset?: number;
  /**
   * 完整行数组的长度（比例分母）。不传时退回 `rows.length`。
   * 分母必须是完整行数，否则窗口滑动会让所有刻度的小数一起漂移，
   * 点击跳转也会落到错误的位置。
   */
  totalRows?: number;
};

export type TurnMark = {
  /** 该轮次在完整会话行数组里的下标（0 起）。 */
  rowIndex: number;
  /** 行号 / 总行数，0..1，与桌面端 `jump-to-turn(fraction)` 的含义一致。 */
  fraction: number;
  /** 便于点击跳转定位的行标识。 */
  rootTurnId: string;
  /** 虚拟行的稳定 key（`data-virtual-key`）。 */
  key: string;
};

/** 桌面端 `timeline.rs:199` 的上限：只保留最新 100 个轮次。 */
export const TURN_RULER_MAX_MARKS = 100;

/** 计算刻度；单趟 O(行数)，调用方放进 `computed` 即可。 */
export const buildTurnMarks = (
  rows: readonly TurnMarkRow[] | null | undefined,
  options: TurnMarkOptions = {}
): TurnMark[] => {
  const list = Array.isArray(rows) ? rows : [];
  const windowLength = list.length;
  const rawTotal = Number(options.totalRows);
  const total = Number.isFinite(rawTotal) && rawTotal > windowLength ? Math.trunc(rawTotal) : windowLength;
  if (total <= 1 || !windowLength) {
    return [];
  }
  const rawOffset = Number(options.rowOffset);
  const offset = Number.isFinite(rawOffset) && rawOffset > 0 ? Math.trunc(rawOffset) : 0;
  const marks: TurnMark[] = [];
  for (let index = 0; index < windowLength; index += 1) {
    const row = list[index];
    if (!row || row.kind === 'greeting' || row.kind === 'spacer') {
      continue;
    }
    const rowIndex = offset + index;
    marks.push({
      rowIndex,
      fraction: Math.min(1, rowIndex / total),
      rootTurnId: String(row.rootTurnId || ''),
      key: String(row.key || '')
    });
  }
  if (marks.length > TURN_RULER_MAX_MARKS) {
    return marks.slice(marks.length - TURN_RULER_MAX_MARKS);
  }
  return marks;
};
