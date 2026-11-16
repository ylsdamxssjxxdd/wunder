/**
 * 工作过程条目的手动开合。命令运行状态不接管展开状态，避免流式输出和
 * 终态到达时反复改变页面高度；用户打开的命令在结束后也保持打开。
 */

const includesKey = (
  keys: ReadonlySet<string> | readonly string[],
  key: string
): boolean =>
  Array.isArray(keys)
    ? keys.includes(key)
    : (keys as ReadonlySet<string>).has(key);

export const resolveEntryOpen = (args: {
  key: string;
  open: ReadonlySet<string> | readonly string[];
}): boolean => includesKey(args.open, args.key);
