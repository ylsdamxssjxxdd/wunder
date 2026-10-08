/**
 * B3 · 时间线条目化：展开上限与有界状态。
 *
 * 规格（方案 §7.6）：
 * - 同时展开的轮次设上限，桌面端实测 `MAX_OPEN_TURNS = 8` 有效，云端沿用；
 * - 超出上限时把最旧一轮折回「已处理」。
 *
 * 这里只保留常量，展开状态由渲染层持有（`MessengerTurnRow` 按轮次位置裁剪，
 * 助手条目区按条目 key 记录），不进入 store，也不复制整段历史。
 */

/** 同时展开的轮次上限（云端沿用桌面端实测值）。 */
export const MAX_OPEN_TURNS = 8;

/** 单个轮次内同时展开的条目上限。 */
export const MAX_OPEN_ENTRIES_PER_TURN = 3;
