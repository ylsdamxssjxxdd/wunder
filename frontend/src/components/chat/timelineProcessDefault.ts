/**
 * 「最近的工作过程默认展开」的判定。
 *
 * 形态基准：桌面端 `crates/wunder-desktop/frontend/src/native_chat_observer.rs`
 * 的 `expand_newest_process` —— 一条线程里只有**最新一条真的带工作过程**的轮次
 * 默认铺开，更早的轮次保持收起。云端版把它拆成两步：
 * 1) 本模块只看数据，回答「这一轮是否有工作过程」「整条线程里最新的是哪一轮」；
 * 2) 渲染层的 `MessageTimelineBlocks` / `MessageTimelineTurnProcess` 收到
 *    `defaultOpen` 后自己铺开（含按需拉取持久条目）。
 *
 * 「有工作过程」的两个来源必须与组件的渲染条件同源，否则会出现「最新一轮被判成
 * 有过程、却没有任何可展开的行」的错位：
 * - 投影块：思考/工具批次栏（`activity`），或过程区外壳（最后一段正文之前还有
 *   内容，见 `MessageTimelineBlocks` 的 `foldEnabled`）；
 * - 旧行 / 线程轻量行：步数或思考文本（`MessageTimelineTurnProcess` 的 `v-if`）。
 */

export type ProcessTurnTimelineBlock = {
  kind?: unknown;
  rows?: unknown;
};

export type ProcessTurnCandidate = {
  key?: unknown;
  assistant?: { message?: Record<string, unknown> | null } | null;
};

const kindOf = (block: ProcessTurnTimelineBlock | undefined): string => String(block?.kind ?? '');

const isBodyBlock = (block: ProcessTurnTimelineBlock | undefined): boolean => kindOf(block) === 'body';

/** 批次栏只有带行时才成行渲染，空批次会在块渲染层被整块丢掉。 */
const isActivityBlock = (block: ProcessTurnTimelineBlock | undefined): boolean => {
  if (kindOf(block) !== 'activity') return false;
  return Array.isArray(block?.rows) ? block.rows.length > 0 : true;
};

/**
 * 是否有「工作过程」外壳（对齐 `MessageTimelineBlocks` 的 `foldEnabled`）：
 * 最后一段正文之前必须还有内容，且折叠体里至少两段或其中含一段正文。
 */
export function timelineHasProcessFold(blocks: unknown): boolean {
  if (!Array.isArray(blocks) || !blocks.length) return false;
  let lastBodyIndex = -1;
  (blocks as ProcessTurnTimelineBlock[]).forEach((block, index) => {
    if (isBodyBlock(block)) lastBodyIndex = index;
  });
  if (lastBodyIndex <= 0) return false;
  if (lastBodyIndex >= 2) return true;
  return (blocks as ProcessTurnTimelineBlock[]).slice(0, lastBodyIndex).some(isBodyBlock);
}

/**
 * 这一轮的投影块里是否存在可展开的工作过程。
 *
 * 单批次轮次（`[批次, 正文]`）没有外壳，批次栏自己就是那一行折叠 —— 它同样是
 * 「工作过程」，最新一轮也要铺开。
 */
export function timelineShowsWorkProcess(blocks: unknown): boolean {
  if (!Array.isArray(blocks) || !blocks.length) return false;
  if ((blocks as ProcessTurnTimelineBlock[]).some(isActivityBlock)) return true;
  return timelineHasProcessFold(blocks);
}

/**
 * 一轮助手消息是否会渲染工作过程。
 *
 * 有投影块时由块列表决定（与 `MessageTimelineAssistant` 的 `hasProjectionBlocks`
 * 分支同源）；没有投影块的旧行/线程轻量行则看步数与思考（与
 * `MessageTimelineTurnProcess` 的 `v-if` 同源）。
 */
export function messageShowsWorkProcess(message: Record<string, unknown> | null | undefined): boolean {
  if (!message || typeof message !== 'object') return false;
  const blocks = message.timeline;
  if (Array.isArray(blocks) && blocks.length) return timelineShowsWorkProcess(blocks);
  const count = Number(message.process_count ?? message.workflow_count) || 0;
  if (count > 0) return true;
  return Boolean(String(message.reasoning ?? '').trim());
}

/**
 * 整条线程里最新一条带工作过程的轮次 key（没有则空串）。
 *
 * 只找**一个**，从末尾往前扫：正常情况第一次比较就命中，长会话也不会退化成
 * 全表遍历。
 */
export function resolveLatestProcessTurnKey(turns: readonly ProcessTurnCandidate[] | null | undefined): string {
  if (!Array.isArray(turns) || !turns.length) return '';
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const turn = turns[index];
    if (!turn) continue;
    if (!messageShowsWorkProcess(turn.assistant?.message)) continue;
    return String(turn.key ?? '');
  }
  return '';
}

/** 某一轮是否是「最近的工作过程」：渲染层据此决定是否默认铺开。 */
export function isLatestProcessTurn(turnKey: unknown, latestProcessTurnKey: unknown): boolean {
  const latest = String(latestProcessTurnKey ?? '');
  return Boolean(latest) && String(turnKey ?? '') === latest;
}
