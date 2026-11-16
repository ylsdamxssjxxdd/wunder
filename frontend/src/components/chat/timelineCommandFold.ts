/**
 * 工作过程里“执行命令”条目的开合口径（桌面版/安卓版同一规则的前端实现）。
 *
 * 命令输出是一行行流进来的：条目在运行中自动展开，让读者直接看到实时输出；
 * 命令结束后自动收起，工作过程重新变回紧凑的一行行。用户只要亲手点过这一条
 * 的展开/收起，就由用户接管，流式帧、翻页与历史刷新都不再改写它。
 *
 * 三个入口分开写，是因为云端版的两个工作过程渲染器（完整投影与轻量线程视图）
 * 各自用 `Set` 或数组保存展开状态；规则本身只有这一份。
 */

/** 判定自动展开所需的最小字段。 */
export type CommandFoldEntry = {
  key: string;
  commandInteraction: string | null;
  status: string;
};

const includesKey = (
  keys: ReadonlySet<string> | readonly string[],
  key: string
): boolean =>
  Array.isArray(keys)
    ? keys.includes(key)
    : (keys as ReadonlySet<string>).has(key);

/** 仍在运行的执行命令条目：这些条目不需要用户点击就先展开。 */
export const autoOpenCommandKeys = (
  entries: Iterable<CommandFoldEntry>
): Set<string> => {
  const keys = new Set<string>();
  for (const entry of entries) {
    if (entry.commandInteraction !== 'execute') continue;
    if (entry.status !== 'loading') continue;
    keys.add(entry.key);
  }
  return keys;
};

/**
 * 一条条目此刻是否展开。
 *
 * - 用户点过的条目：只看用户的展开集合（`open`）；
 * - 其余条目：只看自动集合（`auto`，目前只有运行中的执行命令）。
 */
export const resolveEntryOpen = (args: {
  key: string;
  manual: ReadonlySet<string> | readonly string[];
  open: ReadonlySet<string> | readonly string[];
  auto: ReadonlySet<string> | readonly string[];
}): boolean =>
  includesKey(args.manual, args.key)
    ? includesKey(args.open, args.key)
    : includesKey(args.auto, args.key);
