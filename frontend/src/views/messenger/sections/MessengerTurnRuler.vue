<template>
  <div
    ref="rulerRef"
    v-show="hasStrip"
    class="messenger-turn-ruler"
    aria-hidden="true"
    :data-turn-ruler-strip="`${stripWidth}x${stripHeight}`"
    :data-turn-ruler-span="`${rulerStyle.top};${rulerStyle.left};${rulerStyle.width};${rulerStyle.height}`"
    :data-turn-ruler-marks="marks.length"
    :data-turn-ruler-total="conversationRowCount"
    :data-turn-ruler-height="measureTickHeight()"
    :data-turn-ruler-row-offset="rowOffset"
    :data-turn-ruler-last="lastJumpPath"
    :data-turn-ruler-metrics="`${measureStats.runs}|${measureStats.skipped}|${measureStats.last}`"
    :style="rulerStyle"
    @click="handleStripClick"
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
        '--turn-ruler-tick-top': `${tickTop(index)}px`,
        '--turn-ruler-tick-height': `${tickHitHeight()}px`
      }"
    ></span>
  </div>
</template>

<script setup lang="ts">
/**
 * 聊天区右缘的「用户轮次刻度」（对齐桌面端 `frontend-slint/ui/timeline.slint:481-506`）。
 *
 * 规格（照抄桌面端数字）：
 * - 条体：`x = 容器宽 - 18px`、`y = 10px`、`width = 18px`、`height = 容器高 - 20px`；刻度数 > 1 才可见；
 * - 命中区：宽占满 18px，高 `min(14px, 容器高 / 刻度数)`，`y = (i + 0.5) * 容器高 / 刻度数 - 自身高 / 2`
 *   （**均匀分布**：桌面端注释写明 marathon thread 上按比例摆会挤成一团）；
 * - 刻度线：右对齐（距条体右边 3px），垂直居中，`hover ? 13x3 : 6x2`，圆角 1px，
 *   `hover ? --mz-primary : --mz-border-strong`；悬浮态全部交给 CSS `:hover`，不监听每个刻度。
 *
 * 性能：刻度只由 `buildTurnMarks` 在 `computed` 里算一次（O(轮次数)），滚动回调不参与；
 * 条体几何只用 `ResizeObserver` 量一次（并且只在页签可见时量，避免历史用例断言隐藏元素几何时被强制布局破坏）。
 */
import { computed, onBeforeUnmount, onMounted, ref, unref, watch, type CSSProperties } from 'vue';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import { buildTurnMarks } from '@/views/messenger/timelineTurnMarks';

const props = defineProps<{ controller: MessengerControllerContext }>();

/** 桌面端 `timeline.slint:485-486` 的条体尺寸。 */
const STRIP_WIDTH = 18;
const STRIP_INSET_Y = 10;
/** 桌面端 `timeline.slint:489`：单个命中区高度上限。 */
const MAX_TICK_HIT_HEIGHT = 14;

const caliper = ref({ top: 0, left: 0, width: 0, height: 0 });
/** 最近一次跳转走的是哪条路径：`row` = 目标行在 DOM 里精确居中，`ratio` = 虚拟化退化估算。 */
const lastJumpPath = ref('none');
const stripHeight = computed(() => (caliper.value.height > 0 ? caliper.value.height : 200));
const stripWidth = STRIP_WIDTH;

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
const conversationRows = computed<Array<{ kind?: string; rootTurnId?: string; key?: string }>>(() => {
  const rows = readController<unknown>('agentConversationRows');
  return Array.isArray(rows) ? rows as Array<{ kind?: string; rootTurnId?: string; key?: string }> : [];
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
/** 桌面端 `timeline.slint:486`：刻度数 > 1 才可见。 */
const hasStrip = computed(() => marks.value.length > 1 && caliper.value.width > 0);

const rulerStyle = computed<CSSProperties>(() => ({
  top: `${caliper.value.top + STRIP_INSET_Y}px`,
  left: `${caliper.value.left + Math.max(0, caliper.value.width - STRIP_WIDTH)}px`,
  width: `${STRIP_WIDTH}px`,
  height: `${Math.max(0, caliper.value.height - STRIP_INSET_Y * 2)}px`
}));

/** 命中区高 = `min(14px, 容器高 / 刻度数)`。 */
const tickHitHeight = (): number => {
  const count = marks.value.length;
  if (count <= 0) {
    return MAX_TICK_HIT_HEIGHT;
  }
  return Math.max(2, Math.min(MAX_TICK_HIT_HEIGHT, stripHeight.value / count));
};

/** `y = (i + 0.5) * 容器高 / 刻度数 - 自身高 / 2`（均匀分布，不按小数比例摆）。 */
const tickTop = (index: number): number => {
  const count = marks.value.length;
  if (count <= 0) {
    return 0;
  }
  const slot = stripHeight.value / count;
  return (index + 0.5) * slot - tickHitHeight() / 2;
};

const measureTickHeight = (): number => Math.round(tickHitHeight() * 100) / 100;

// --- 几何测量：只在页签可见时量，避免隐藏状态下的强制布局 ---------------

const rulerRef = ref<HTMLElement | null>(null);
/**
 * 几何测点（`data-turn-ruler-metrics` = `测量次数|跳过次数|最近一次读数`）。
 * 条体的挂点/尺寸依赖运行期布局，这个属性是真机上定位「刻度条没出现 / 量到 0」的第一手证据。
 */
const measureStats = ref({ runs: 0, skipped: 0, last: '' });
let measureFrame: number | null = null;
let resizeObserver: ResizeObserver | null = null;

const measureStrip = (): void => {
  const container = resolveScrollContainer();
  const main = rulerRef.value?.parentElement || null;
  if (!container || !main || typeof document === 'undefined' || document.hidden) {
    measureStats.value = {
      runs: measureStats.value.runs + 1,
      skipped: measureStats.value.skipped + 1,
      last: 'skipped'
    };
    return;
  }
  const containerRect = container.getBoundingClientRect();
  const mainRect = main.getBoundingClientRect();
  const next = {
    top: containerRect.top - mainRect.top,
    left: containerRect.left - mainRect.left,
    width: containerRect.width,
    height: containerRect.height
  };
  measureStats.value = {
    runs: measureStats.value.runs + 1,
    skipped: measureStats.value.skipped,
    last: `${Math.round(next.left)},${Math.round(next.top)} ${Math.round(next.width)}x${Math.round(next.height)}`
  };
  const current = caliper.value;
  if (
    Math.abs(current.top - next.top) < 0.5 &&
    Math.abs(current.left - next.left) < 0.5 &&
    Math.abs(current.width - next.width) < 0.5 &&
    Math.abs(current.height - next.height) < 0.5
  ) {
    return;
  }
  caliper.value = next;
};

const scheduleMeasure = (): void => {
  if (measureFrame !== null || typeof window === 'undefined') {
    return;
  }
  measureFrame = window.requestAnimationFrame(() => {
    measureFrame = null;
    measureStrip();
  });
};

const handleDocumentVisibility = (): void => {
  if (!document.hidden) {
    scheduleMeasure();
  }
};

onMounted(() => {
  scheduleMeasure();
  // 观察父层与滚动容器本身：条体的高度是从滚动容器量出来的，
  // 只观察条体自己会陷入「高 0 → 条体高 0 → 不再触发观察」的死循环。
  if (typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver(scheduleMeasure);
    if (rulerRef.value?.parentElement) {
      resizeObserver.observe(rulerRef.value.parentElement);
    }
    const container = resolveScrollContainer();
    if (container) {
      resizeObserver.observe(container);
    }
  }
  document.addEventListener('visibilitychange', handleDocumentVisibility);
});

// 轮次结构变化（新会话 / 新轮次 / 切换线程）后重新量一次几何。
watch(marks, scheduleMeasure);

onBeforeUnmount(() => {
  if (measureFrame !== null && typeof window !== 'undefined') {
    window.cancelAnimationFrame(measureFrame);
    measureFrame = null;
  }
  resizeObserver?.disconnect();
  resizeObserver = null;
  document.removeEventListener('visibilitychange', handleDocumentVisibility);
});

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

const handleStripClick = (event: MouseEvent): void => {
  const count = marks.value.length;
  if (count <= 0) {
    return;
  }
  // 命中区按条体高度均分（与桌面端逐个 TouchArea 的槽位一致）。
  const height = stripHeight.value;
  const slot = height / count;
  if (!(slot > 0)) {
    return;
  }
  const strip = rulerRef.value;
  const top = strip ? strip.getBoundingClientRect().top : 0;
  const index = Math.min(count - 1, Math.max(0, Math.floor((event.clientY - top) / slot)));
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

/**
 * 桌面端在刻度条上滚轮时把手势还给聊天（`timeline.slint:503` 明确 reject）。
 * 条体挂在滚动容器**外面**，事件不会冒泡到它，因此显式转发一次并让事件继续冒泡。
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
/* 条体本身不可见：只有刻度线。挂点是 `.messenger-main`（相对定位），
   因此 `.messenger-chat-body` 滚动时刻度条不会跟着内容滚走。 */
.messenger-turn-ruler {
  position: absolute;
  z-index: 6;
  pointer-events: auto;
}

/* 命中区：宽度占满 18px（`left: 0; right: 0`），高度与 y 由内联样式给出。
   悬浮放大/变色只用 CSS `:hover`，不用 JS 监听每个刻度。 */
.messenger-turn-ruler-tick {
  position: absolute;
  top: var(--turn-ruler-tick-top, 0);
  right: 0;
  left: 0;
  /* 空 span 的 auto 高度是 0，命中区高度必须显式给（桌面端用固定 height 的 Rectangle）。 */
  height: var(--turn-ruler-tick-height, 2px);
  border-radius: 1px;
  cursor: pointer;
}

/* 刻度线：右对齐（距条体右缘 3px）、垂直居中，默认 6x2px / --mz-border-strong。 */
.messenger-turn-ruler-tick::after {
  content: '';
  position: absolute;
  top: 50%;
  right: 3px;
  width: 6px;
  height: 2px;
  border-radius: 1px;
  background: var(--mz-border-strong, #d8d5d0);
  transform: translateY(-50%);
  transition:
    width 120ms ease,
    height 120ms ease,
    background-color 120ms ease;
}

/* 悬浮：13x3px / --mz-primary（桌面端 Theme.accent）。 */
.messenger-turn-ruler-tick:hover::after {
  width: 13px;
  height: 3px;
  background: var(--mz-primary, #c96443);
}
</style>
