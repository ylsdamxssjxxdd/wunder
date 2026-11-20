/**
 * 覆盖式细滚动条几何。
 *
 * 云端版全局隐藏了原生滚动条（见 styles/base.css），需要滚动提示的容器用
 * ThinScrollbar 覆盖层自绘细滑块。这里只做纯几何换算：把一个滚动容器沿某一轴
 * 的「视口尺寸 / 内容尺寸 / 当前偏移」换算成轨道上的滑块尺寸与偏移。
 *
 * 约定：
 * - 滑块尺寸与偏移都是轨道坐标系里的像素值，轨道长度由调用方给出；
 * - 内容不溢出时 scrollable=false，滑块铺满轨道，调用方据此隐藏；
 * - 偏移在拖动过程中可能越界（滚动容器的 scrollTop 会被浏览器夹紧），
 *   因此比例始终夹到 [0, 1]，避免滑块跑出轨道。
 */

export type OverlayScrollbarMetrics = {
  /** 内容是否超出视口；为 false 时没有滚动意义。 */
  scrollable: boolean;
  /** 滑块沿轴长度（px）。 */
  thumbSize: number;
  /** 滑块沿轴起点（px），相对轨道起点。 */
  thumbOffset: number;
};

export type OverlayScrollbarInput = {
  /** 视口长度：clientHeight / clientWidth。 */
  viewport: number;
  /** 内容长度：scrollHeight / scrollWidth。 */
  content: number;
  /** 当前偏移：scrollTop / scrollLeft。 */
  offset: number;
  /** 轨道长度（px）。 */
  track: number;
  /** 滑块最短长度（px），避免内容很长时滑块细到抓不住。 */
  minThumb?: number;
};

const DEFAULT_MIN_THUMB = 28;
/** 亚像素级的溢出不算滚动：滚动容器取整后经常差 0.5px。 */
const MIN_SCROLL_RANGE = 0.5;

function finite(value: number): number {
  return Number.isFinite(value) && value > 0 ? value : 0;
}

export function overlayScrollbarMetrics(input: OverlayScrollbarInput): OverlayScrollbarMetrics {
  const track = finite(input.track);
  if (track <= 0) return { scrollable: false, thumbSize: 0, thumbOffset: 0 };

  const viewport = finite(input.viewport);
  const content = Math.max(viewport, finite(input.content));
  const range = content - viewport;
  if (range <= MIN_SCROLL_RANGE || viewport <= 0) {
    return { scrollable: false, thumbSize: Math.round(track), thumbOffset: 0 };
  }

  const minThumb = Math.min(finite(input.minThumb ?? DEFAULT_MIN_THUMB) || DEFAULT_MIN_THUMB, track);
  const ratio = Math.min(1, Math.max(0, viewport / content));
  const thumbSize = Math.max(minThumb, Math.min(track, Math.round(track * ratio)));
  const maxOffset = Math.max(0, track - thumbSize);
  const progress = Math.min(1, Math.max(0, finite(input.offset) / range));
  return {
    scrollable: true,
    thumbSize,
    thumbOffset: Math.round(maxOffset * progress),
  };
}

/**
 * 拖动反解：滑块移动 delta 像素时需要给滚动容器加多少偏移量。
 * 轨道被滑块自身占去一部分，因此以「可移动轨道」为分母。
 */
export function overlayScrollbarDragOffset(input: {
  delta: number;
  track: number;
  thumbSize: number;
  viewport: number;
  content: number;
}): number {
  const range = Math.max(0, finite(input.content) - finite(input.viewport));
  const maxOffset = Math.max(0, finite(input.track) - finite(input.thumbSize));
  if (maxOffset <= 0 || range <= 0) return 0;
  return (input.delta * range) / maxOffset;
}
