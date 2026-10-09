<template>
  <div
    ref="rulerRef"
    v-show="hasStrip"
    class="messenger-turn-ruler"
    aria-hidden="true"
    :data-turn-ruler-marks="marks.length"
    :data-turn-ruler-total="conversationRowCount"
    :data-turn-ruler-last="lastJumpPath"
    :style="{ '--turn-ruler-slot-count': Math.max(1, marks.length), '--turn-ruler-slot-max': `${SLOT_MAX_PX}px` }"
    @click="handleStripClick"
    @mousemove="handleStripHover"
    @mouseleave="hoverIndex = -1"
    @wheel="handleStripWheel"
  >
    <span
      v-for="(mark, index) in marks"
      :key="mark.key || `turn-mark-${mark.rowIndex}`"
      class="messenger-turn-ruler-tick"
      :data-turn-ruler-tick="index"
      :data-turn-ruler-turn-id="mark.rootTurnId"
      :data-turn-ruler-row="mark.rowIndex"
      :style="{
        '--turn-ruler-tick-offset': `${index + 0.5 - marks.length / 2}`,
        '--turn-ruler-tick-w': `${tickWidth(index)}px`
      }"
    ></span>
    <!-- 悬停预览卡：标题取该轮次的用户消息，摘要取助手回复（参考主页面刻度交互）。 -->
    <div
      v-if="hoverMark && (hoverTitle || hoverSnippet)"
      class="messenger-turn-ruler-preview"
      :data-turn-ruler-preview="hoverIndex"
      :style="{ '--turn-ruler-preview-offset': previewOffset }"
    >
      <div v-if="hoverTitle" class="messenger-turn-ruler-preview-title">{{ hoverTitle }}</div>
      <div v-if="hoverSnippet" class="messenger-turn-ruler-preview-body">{{ hoverSnippet }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 聊天区右缘的「用户轮次刻度」（对齐桌面端 `frontend-slint/ui/timeline.slint:541-566`）。
 *
 * 布局：组件是 `.messenger-chat-lane` 里滚动容器的**同级 flex 通道**，独占 18px 宽
 * （桌面端 marks-strip 同款独占矩形），上下留 10px——空间由布局分配，不依赖
 * 运行期测量，也不会压到滚动条上。
 *
 * 规格：
 * - 刻度数 > 1 才可见；
 * - 紧凑排布：槽位间距 `min(条体高 / 刻度数, 12px)`，刻度组贴成一列、围绕条体
 *   垂直中点居中（刻度极多时退回全高均分避免溢出）；
 * - 波感：悬停时以所在刻度为中心向外宽度递减（14 → 11 → 8 → 6px），宽度由
 *   `hoverIndex` 的距离算出并经 CSS transition 平滑，形成从中心向外的波浪；
 * - 刻度线：右对齐（距条体右边 3px），垂直居中，悬停刻度 3px 高 / --mz-primary，
 *   其余 2px / --mz-border-strong；点击/悬停槽位映射与 CSS 同式。
 *
 * 性能：刻度只由 `buildTurnMarks` 在 `computed` 里算一次（O(轮次数)），滚动回调不参与；
 * 悬停只更新 `hoverIndex` 一个 ref（≤100 个刻度的宽度样式），没有几何测量、
 * 没有 ResizeObserver、没有任何滚动监听。
 */
import { computed, ref, unref } from 'vue';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import { buildTurnMarks } from '@/views/messenger/timelineTurnMarks';

const props = defineProps<{ controller: MessengerControllerContext }>();

/** 槽位间距上限：刻度紧贴成一列、围绕条体中点居中，极多刻度时退回全高均分。 */
const SLOT_MAX_PX = 12;
/** 波感宽度：悬停刻度最长，相邻按 3px 递减，最近的保底 6px。 */
const TICK_WAVE_MAX_PX = 14;
const TICK_WAVE_STEP_PX = 3;
const TICK_WAVE_MIN_PX = 6;

/**
 * 控制器里的响应式字段可能是 ref，也可能被外层拆成普通值；
 * 脚本侧统一用 unref 取值，避免两种装配方式下行为不一致。
 */
const readController = <T,>(key: string): T | undefined => unref(props.controller?.[key]) as T | undefined;

const resolveScrollContainer = (): HTMLElement | null =>
  (readController<HTMLElement | null>('messageListRef') || null);

/**
 * 会话行：每行 = 一个用户轮次（`agentConversationRows`）。
 * 虚拟化下 `agentVirtualRows` 只是窗口，单看窗口长度会让刻度比例随滚动漂移，
 * 所以这里取「完整行数」当分母、并用窗口起点还原绝对行号——与桌面端
 * `timeline.rs:187-203` 的「行号 / 总行数」口径一致。
 */
const conversationRows = computed<Array<{
  kind?: string;
  rootTurnId?: string;
  key?: string;
  user?: { message?: Record<string, unknown> };
  assistant?: { message?: Record<string, unknown> };
}>>(() => {
  const rows = readController<unknown>('agentConversationRows');
  return Array.isArray(rows)
    ? rows as Array<{
      kind?: string;
      rootTurnId?: string;
      key?: string;
      user?: { message?: Record<string, unknown> };
      assistant?: { message?: Record<string, unknown> };
    }>
    : [];
});
const conversationRowCount = computed(() => conversationRows.value.length);
const rowOffset = computed(() => {
  const start = readController<{ startIndex?: number }>('agentVirtualWindow')?.startIndex;
  return Number.isFinite(start) && Number(start) > 0 ? Math.trunc(Number(start)) : 0;
});
const marks = computed(() => buildTurnMarks(conversationRows.value, {
  rowOffset: rowOffset.value,
  totalRows: conversationRowCount.value
}));
/** 桌面端 `timeline.slint:546`：刻度数 > 1 才可见。 */
const hasStrip = computed(() => marks.value.length > 1);

/** 最近一次跳转走的是哪条路径：`row` = 目标行在 DOM 里精确居中，`ratio` = 虚拟化退化估算。 */
const lastJumpPath = ref('none');
const rulerRef = ref<HTMLElement | null>(null);

// --- 交互 ---------------------------------------------------------------

/**
 * 轮次高度量程：虚拟化后目标行不在 DOM 里时用它退化为「按测量高度估算 scrollTop」，
 * 避免只按列数估算把长会话的目标行落在视口外。只在点击时扫一次可见行，滚动路径不碰。
 */
const findTurnNode = (container: HTMLElement, turnId: string): HTMLElement | null => {
  if (!turnId) {
    return null;
  }
  const escaped = typeof CSS !== 'undefined' && typeof CSS.escape === 'function'
    ? CSS.escape(turnId)
    : turnId.replace(/["\\]/g, '\\$&');
  return container.querySelector<HTMLElement>(`.messenger-turn[data-root-turn-id="${escaped}"]`);
};

const collectMeasuredRowHeights = (): number[] => {
  const container = resolveScrollContainer();
  const rows = conversationRows.value;
  if (!container || !rows.length) {
    return [];
  }
  const heights = new Array<number>(rows.length).fill(0);
  const indexByTurnId = new Map<string, number>();
  rows.forEach((row: { rootTurnId?: string }, index: number) => {
    const turnId = String(row?.rootTurnId || '');
    if (turnId) {
      indexByTurnId.set(turnId, index);
    }
  });
  container.querySelectorAll<HTMLElement>('.messenger-turn[data-root-turn-id]').forEach((node) => {
    const index = indexByTurnId.get(String(node.dataset.rootTurnId || ''));
    const height = Math.round(node.offsetHeight || 0);
    if (index === undefined || height <= 0) {
      return;
    }
    heights[index] = height;
  });
  return heights;
};

const scrollContainerTo = (top: number): void => {
  const container = resolveScrollContainer();
  if (!container) {
    return;
  }
  const maxTop = Math.max(0, container.scrollHeight - container.clientHeight);
  const nextTop = Math.min(Math.max(0, Math.round(top)), maxTop);
  container.scrollTop = nextTop;
  const onScroll = readController<(event?: Event) => void>('handleMessageListScroll');
  if (typeof onScroll === 'function') {
    // 复用现有滚动链路：更新跟随态 / 虚拟窗口 / 记忆位（不新增 scroll 监听）。
    onScroll();
  }
};

const requestAnimationFrameSafe = (run: () => void): void => {
  if (typeof window === 'undefined') {
    return;
  }
  window.requestAnimationFrame(run);
};

/**
 * 把目标轮次落到视口中间。
 *
 * 先量后滚：滚动会让虚拟窗口换行、消息布局回流（行高变化），所以按帧重试到收敛
 * （最多 4 帧，正常情况第二帧就 0 偏差）。行不在 DOM 里时退化为按测量高度估算的
 * `scrollVirtualMessageToIndex` 路径；实际走哪条记在 `data-turn-ruler-last` 上。
 */
const centerTurnRow = (rowIndex: number, turnId: string, attempt = 0): void => {
  const container = resolveScrollContainer();
  if (!container) {
    return;
  }
  if (typeof window === 'undefined') {
    return;
  }

  if (turnId) {
    const node = findTurnNode(container, turnId);
    if (node) {
      lastJumpPath.value = 'row';
      const containerRect = container.getBoundingClientRect();
      const nodeRect = node.getBoundingClientRect();
      const nodeCenter = nodeRect.top - containerRect.top + nodeRect.height / 2;
      const delta = nodeCenter - container.clientHeight / 2;
      if (Math.abs(delta) <= 1) {
        return;
      }
      scrollContainerTo(container.scrollTop + delta);
      if (attempt < 3) {
        requestAnimationFrameSafe(() => centerTurnRow(rowIndex, turnId, attempt + 1));
      }
      return;
    }
  }

  // 退化路径：目标行被虚拟化卸载，按已测量高度估算偏移后置中。
  lastJumpPath.value = 'ratio';
  const keys = conversationRows.value
    .map((row) => String(row?.key || ''))
    .filter(Boolean);
  const scrollToIndex = readController<(keys: string[], index: number, align?: string) => void>(
    'scrollVirtualMessageToIndex'
  );
  if (keys.length > 1 && typeof scrollToIndex === 'function') {
    // 运行时（messageViewportRuntime）的键序是「每个轮次两条消息」，轮次下标 -> 首条消息下标。
    scrollToIndex(keys, Math.min(keys.length - 1, rowIndex * 2), 'center');
  }
  const heights = collectMeasuredRowHeights();
  if (!heights.length || !heights.some((height) => height > 0)) {
    return;
  }
  const estimateRowHeight = unref(props.controller?.resolveVirtualMessageHeight) as
    | ((key: string) => number)
    | undefined;
  const rowHeight = (index: number): number => {
    const measured = heights[index] || 0;
    if (measured > 0) {
      return measured;
    }
    const key = keys[index] || '';
    const resolved = typeof estimateRowHeight === 'function' ? Number(estimateRowHeight(key)) : 0;
    // 运行时的键是「轮次里的单条消息」，一个轮次 = 两条消息。
    return resolved > 0 ? resolved * 2 : 0;
  };
  let offset = 0;
  for (let index = 0; index < Math.min(rowIndex, heights.length); index += 1) {
    offset += rowHeight(index);
  }
  const targetHeight = rowHeight(Math.min(rowIndex, heights.length - 1));
  scrollContainerTo(offset - container.clientHeight / 2 + targetHeight / 2);
};

/**
 * 槽位算法与 CSS 同式：间距 `min(条体高 / 刻度数, SLOT_MAX_PX)`，刻度组以条体中点
 * 为中心排开；组外空白（上下两端的留空）就近夹到首/末刻度。
 */
const resolveSlotIndex = (clientY: number): number => {
  const strip = rulerRef.value;
  if (!strip) {
    return -1;
  }
  const count = marks.value.length;
  if (count <= 0) {
    return -1;
  }
  const rect = strip.getBoundingClientRect();
  const slot = Math.min(rect.height / count, SLOT_MAX_PX);
  if (!(slot > 0)) {
    return -1;
  }
  const start = (rect.height - slot * count) / 2;
  const position = clientY - rect.top - start;
  return Math.min(count - 1, Math.max(0, Math.floor(position / slot)));
};

const handleStripClick = (event: MouseEvent): void => {
  const index = resolveSlotIndex(event.clientY);
  if (index < 0) {
    return;
  }
  const mark = marks.value[index];
  if (!mark) {
    return;
  }
  // 桌面端 jump-to-turn 会先退出「跟随输出」；蜂巢的跟随态由 handleMessageListScroll 统一判定。
  const autoStick = props.controller?.autoStickToBottom;
  if (autoStick && typeof autoStick === 'object' && 'value' in autoStick) {
    (autoStick as { value: boolean }).value = false;
  }
  centerTurnRow(mark.rowIndex, mark.rootTurnId);
};

// --- 悬停预览 -------------------------------------------------------------

const hoverIndex = ref(-1);
const hoverMark = computed(() => (hoverIndex.value >= 0 ? marks.value[hoverIndex.value] || null : null));

/** 行内容按 mark.key 查找：rowIndex 在窗口装配下未必等于完整数组下标，key 是稳定标识。 */
const rowsByKey = computed(() => {
  const map = new Map<string, (typeof conversationRows.value)[number]>();
  conversationRows.value.forEach((row) => {
    const key = String(row?.key || '');
    if (key && !map.has(key)) {
      map.set(key, row);
    }
  });
  return map;
});

const messageText = (slot: { message?: Record<string, unknown> } | undefined): string => {
  const content = slot?.message?.content;
  return String(typeof content === 'string' ? content : '').replace(/\s+/g, ' ').trim();
};

const hoverTitle = computed(() => {
  const row = hoverMark.value ? rowsByKey.value.get(hoverMark.value.key) : undefined;
  return row ? messageText(row.user).slice(0, 160) : '';
});

const hoverSnippet = computed(() => {
  const row = hoverMark.value ? rowsByKey.value.get(hoverMark.value.key) : undefined;
  return row ? messageText(row.assistant).slice(0, 200) : '';
});

/** 预览卡垂直位置 = 悬停刻度槽位中心相对条体中点的偏移（槽位数），clamp 交给 CSS。 */
const previewOffset = computed(() => `${Math.max(0, hoverIndex.value) + 0.5 - marks.value.length / 2}`);

const handleStripHover = (event: MouseEvent): void => {
  hoverIndex.value = resolveSlotIndex(event.clientY);
};

/**
 * 波感宽度：悬停刻度最长（TICK_WAVE_MAX_PX），按距离每格递减 STEP，保底 MIN；
 * 未悬停时全部回到基础宽度。宽度变化经 CSS transition 平滑。
 */
const tickWidth = (index: number): number => {
  if (hoverIndex.value < 0) {
    return TICK_WAVE_MIN_PX;
  }
  const distance = Math.abs(index - hoverIndex.value);
  return Math.max(
    TICK_WAVE_MIN_PX,
    TICK_WAVE_MAX_PX - distance * TICK_WAVE_STEP_PX
  );
};

/**
 * 桌面端在刻度条上滚轮时把手势还给聊天（`timeline.slint:563` 明确 reject）。
 * 通道在滚动容器外面，事件不会冒泡到它，因此显式转发一次。
 */
const handleStripWheel = (event: WheelEvent): void => {
  const container = resolveScrollContainer();
  if (!container) {
    return;
  }
  const delta = event.deltaMode === 1 ? event.deltaY * 16 : event.deltaY;
  if (!delta) {
    return;
  }
  const maxTop = Math.max(0, container.scrollHeight - container.clientHeight);
  container.scrollTop = Math.min(maxTop, Math.max(0, container.scrollTop + delta));
  const onScroll = readController<(event?: Event) => void>('handleMessageListScroll');
  if (typeof onScroll === 'function') {
    onScroll();
  }
};
</script>

<style scoped>
/* 通道本体：`.messenger-chat-lane` 里的固定 18px 列，上下留 10px。
   没有刻度时 `v-show` 收起，flex 布局自动把空间还给滚动视口。
   槽位间距 = `min(条体高 / 刻度数, 12px)`：刻度贴成一列、围绕条体垂直中点居中，
   刻度极多时间距被条体高度压小（退回全高均分），不会溢出。 */
.messenger-turn-ruler {
  --turn-ruler-slot: min(
    calc(100% / var(--turn-ruler-slot-count, 1)),
    var(--turn-ruler-slot-max, 12px)
  );
  position: relative;
  flex-shrink: 0;
  box-sizing: border-box;
  width: 18px;
  margin: 10px 0;
  z-index: 6;
}

/* 命中区：宽度占满 18px（`left: 0; right: 0`），高 = 槽位间距（贴在一起），
   中心在条体中点两侧按 `--turn-ruler-tick-offset`（槽位数）排开。
   刻度线宽度由 `--turn-ruler-tick-w`（波感）给出，悬浮变色只用 CSS `:hover`。 */
.messenger-turn-ruler-tick {
  position: absolute;
  top: calc(50% + var(--turn-ruler-tick-offset, 0) * var(--turn-ruler-slot));
  right: 0;
  left: 0;
  height: var(--turn-ruler-slot);
  transform: translateY(-50%);
  cursor: pointer;
}

/* 刻度线：右对齐（距条体右缘 3px）、垂直居中，宽度 = 波感值，默认 2px 高。 */
.messenger-turn-ruler-tick::after {
  content: '';
  position: absolute;
  top: 50%;
  right: 3px;
  width: var(--turn-ruler-tick-w, 6px);
  height: 2px;
  border-radius: 1px;
  background: var(--mz-border-strong, #d8d5d0);
  transform: translateY(-50%);
  transition:
    width 120ms ease,
    height 120ms ease,
    background-color 120ms ease;
}

/* 悬停刻度：加高并染主色（宽度波感由 JS 距离给出）。 */
.messenger-turn-ruler-tick:hover::after {
  height: 3px;
  background: var(--mz-primary, #c96443);
}

/* 悬停预览卡：贴在刻度通道左侧，垂直位置由 `--turn-ruler-preview-offset`
   （槽位中心相对条体中点的槽位数 × 槽位间距）给出，clamp 在通道范围内避免溢出。
   pointer-events 关掉，避免卡片盖住聊天内容时抢走悬停/点击。 */
.messenger-turn-ruler-preview {
  position: absolute;
  top: clamp(
    48px,
    calc(50% + var(--turn-ruler-preview-offset, 0) * var(--turn-ruler-slot)),
    calc(100% - 48px)
  );
  right: calc(100% + 12px);
  transform: translateY(-50%);
  box-sizing: border-box;
  width: 264px;
  padding: 10px 12px;
  border: 1px solid var(--mz-border, #e3e0db);
  border-radius: 10px;
  background: var(--mz-surface, #ffffff);
  box-shadow: 0 8px 24px rgba(31, 35, 41, 0.12);
  pointer-events: none;
  z-index: 7;
}

.messenger-turn-ruler-preview-title {
  display: -webkit-box;
  overflow: hidden;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  color: var(--mz-text, #1f2329);
  font-size: 13px;
  font-weight: 600;
  line-height: 1.45;
  word-break: break-word;
}

.messenger-turn-ruler-preview-body {
  display: -webkit-box;
  overflow: hidden;
  margin-top: 4px;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 3;
  color: var(--mz-text-secondary, #6b7280);
  font-size: 12px;
  line-height: 1.5;
  word-break: break-word;
}
</style>
