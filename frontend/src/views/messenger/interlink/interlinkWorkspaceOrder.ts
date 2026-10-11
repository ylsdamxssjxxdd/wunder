// AI生成
/**
 * 侧栏工作区行的云端排序（互通 §6.3）。
 *
 * 顺序是蜂巢自己的账号级偏好：只重排蜂巢侧栏的行渲染，不回写本地设备的
 * `sort_index`，也不影响影子投影的内容。存储形态是一份行 key 的有序列表；
 * 没记过序的行按自然顺序排在后面，列表里已消失的 key 直接忽略。
 */

import type { InterlinkWorkspaceRow } from './interlinkNodeModel';

// 行 key 与有序列表的存取（GET/PUT /wunder/interlink/workspace-order 及其上限
// 归一化）在 `@/api/interlink` 的 fetch/saveInterlinkWorkspaceOrder。

/**
 * 按 saved 顺序重排行；saved 里没有的行保持自然顺序排在末尾，
 * saved 里已不存在的 key 丢弃。两个输入都不修改。
 */
export const applyWorkspaceOrder = (
  rows: InterlinkWorkspaceRow[],
  saved: string[]
): InterlinkWorkspaceRow[] => {
  if (!rows.length) return rows;
  const byKey = new Map(rows.map((row) => [row.key, row]));
  const ordered: InterlinkWorkspaceRow[] = [];
  const consumed = new Set<string>();
  saved.forEach((key) => {
    const row = byKey.get(key);
    if (!row || consumed.has(key)) return;
    consumed.add(key);
    ordered.push(row);
  });
  rows.forEach((row) => {
    if (!consumed.has(row.key)) ordered.push(row);
  });
  return ordered;
};

/**
 * 拖放后写回的完整顺序：被拖行落到目标行的原槽位（目标行及其后整体后移），
 * 其余行保持当前相对顺序。无变化时返回 null（调用方跳过保存）。
 */
export const moveWorkspaceRow = (
  keys: string[],
  draggedKey: string,
  targetKey: string
): string[] | null => {
  const from = keys.indexOf(draggedKey);
  const to = keys.indexOf(targetKey);
  if (from < 0 || to < 0 || from === to) return null;
  const next = keys.slice();
  next.splice(from, 1);
  next.splice(to, 0, draggedKey);
  return next;
};
