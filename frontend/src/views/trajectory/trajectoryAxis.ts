/**
 * Shared trajectory axis model.
 * TS port of crates/wunder-core/src/trajectory_axis.rs and
 * web/shared/trajectory-axis.js — keep all three in sync.
 * Sequence labels are event ordinals (`#N`); duration labels are offsets from
 * the domain origin (`Δt`), never absolute Unix timestamps.
 */

export const TICK_TARGET_PX = 120
export const MIN_LABEL_GAP_PX = 56
export const MIN_TURN_GAP_PX = 48
export const MIN_SPAN_PX = 2

export type AxisMode = 'sequence' | 'duration'

export interface AxisTick {
  value: number
  x: number
  label: string
}

export interface TurnTick {
  turn: number
  x: number
  label: string
}

export interface PixelSpan {
  left: number
  right: number
  lane: number
  error: boolean
  selected: boolean
  /** Search/selection state; a folded mark stays bright when any member matches. */
  dimmed: boolean
  outside: boolean
  index: number
}

export interface FoldedSpan {
  left: number
  right: number
  lane: number
  error: boolean
  selected: boolean
  dimmed: boolean
  outside: boolean
  index: number
  count: number
}

const DURATION_STEPS_MS = [
  1, 2, 5, 10, 20, 50, 100, 200, 500,
  1000, 2000, 5000, 10000, 15000, 30000,
  60000, 120000, 300000, 600000, 900000,
  1800000, 3600000, 7200000, 10800000, 21600000,
  43200000, 86400000, 172800000, 604800000, 2592000000,
]
const DAY_MS = 86400000

export function axisUnit(mode: AxisMode): string {
  return mode === 'sequence' ? '#E' : 'Δt'
}

const finite = (value: number, fallback: number): number => Number.isFinite(value) ? value : fallback
const trim = (value: number): number => Math.abs(value) < 1e-6 ? 0 : value
const pad2 = (value: number): string => String(value).padStart(2, '0')

/** Smallest human step that is at least `raw`. */
export function niceStep(raw: number, mode: AxisMode): number {
  const value = Math.max(Number.MIN_VALUE, finite(raw, 1))
  if (mode === 'sequence') {
    const magnitude = 10 ** Math.floor(Math.log10(value))
    const residual = value / magnitude
    const mantissa = residual > 5 ? 10 : residual > 2 ? 5 : residual > 1 ? 2 : 1
    return mantissa * magnitude
  }
  return DURATION_STEPS_MS.find(step => step >= value)
    ?? Math.ceil(value / DAY_MS) * DAY_MS
}

/** Next step on the same ladder, used to widen labels until they fit. */
export function nextNiceStep(step: number, mode: AxisMode): number {
  if (mode === 'sequence') {
    const magnitude = 10 ** Math.floor(Math.log10(Math.max(Number.MIN_VALUE, step)))
    const residual = Math.round(step / magnitude)
    const next = [1, 2, 5].find(m => m > residual + 1e-9) ?? 10
    return next * magnitude
  }
  return DURATION_STEPS_MS.find(candidate => candidate > step + 1e-9) ?? step + DAY_MS
}

/** Compact, language-neutral offset label. */
export function formatOffset(ms: number): string {
  const value = Math.abs(trim(ms))
  if (value < 1000) return `${Math.round(value)}ms`
  if (value < 60000) {
    const seconds = value / 1000
    return value < 10000 ? `${seconds.toFixed(1)}s` : `${Math.round(seconds)}s`
  }
  if (value < 3600000) {
    const minutes = Math.floor(value / 60000)
    const seconds = Math.round((value % 60000) / 1000)
    return seconds > 0 ? `${minutes}m ${pad2(seconds)}s` : `${minutes}m`
  }
  if (value < DAY_MS) {
    const hours = Math.floor(value / 3600000)
    const minutes = Math.round((value % 3600000) / 60000)
    return minutes > 0 ? `${hours}h ${pad2(minutes)}m` : `${hours}h`
  }
  const days = Math.floor(value / DAY_MS)
  const hours = Math.round((value % DAY_MS) / 3600000)
  return hours > 0 ? `${days}d ${pad2(hours)}h` : `${days}d`
}

export function formatLabel(offset: number, mode: AxisMode): string {
  return mode === 'sequence' ? `#${Math.round(trim(offset))}` : formatOffset(offset)
}

/** Ticks aligned to the domain origin, never to absolute Unix time. */
export function axisTicks(options: {
  domainStart: number
  viewStart: number
  viewSpan: number
  width: number
  mode: AxisMode
}): AxisTick[] {
  const { domainStart, viewStart } = options
  const width = Math.max(0, finite(options.width, 0))
  const span = Math.max(Number.MIN_VALUE, finite(options.viewSpan, 0))
  if (width <= 0 || !Number.isFinite(viewStart) || !Number.isFinite(domainStart)) return []
  const target = Math.max(2, Math.floor(width / TICK_TARGET_PX))
  let step = niceStep(span / target, options.mode)
  for (let i = 0; i < 32; i += 1) {
    if (step * width / span >= MIN_LABEL_GAP_PX || step >= span) break
    const next = nextNiceStep(step, options.mode)
    if (next <= step) break
    step = next
  }
  const relativeStart = trim(viewStart - domainStart)
  const firstOffset = Math.ceil(relativeStart / step) * step
  const endOffset = relativeStart + span
  const ticks: AxisTick[] = []
  let offset = firstOffset
  for (let i = 0; i < 512 && offset <= endOffset + step * 1e-9; i += 1, offset += step) {
    const value = domainStart + offset
    const x = (value - viewStart) / span * width
    if (x >= -0.5 && x <= width + 0.5) {
      ticks.push({ value, x, label: formatLabel(offset, options.mode) })
    }
  }
  return ticks
}

/** Thin and label turn markers; input is [{turn, x}] in pixels. */
export function thinTurns(turns: ReadonlyArray<{ turn: number; x: number }>, width: number): TurnTick[] {
  const w = Math.max(0, finite(width, 0))
  if (w <= 0 || turns.length === 0) return []
  const candidates = turns
    .filter(item => Number.isFinite(item.x) && item.x >= 0 && item.x <= w)
    .map(item => ({ turn: Math.round(item.turn), x: item.x }))
    .sort((a, b) => a.x - b.x)
  const capacity = Math.max(1, Math.floor(w / MIN_TURN_GAP_PX))
  let cadence = 1
  if (candidates.length > capacity) {
    const ratio = candidates.length / capacity
    const magnitude = 10 ** Math.floor(Math.log10(ratio))
    const residual = ratio / magnitude
    cadence = Math.max(1, Math.round((residual > 5 ? 10 : residual > 2 ? 5 : residual > 1 ? 2 : 1) * magnitude))
  }
  const result: TurnTick[] = []
  let lastX = -Infinity
  for (const { turn, x } of candidates) {
    if (cadence > 1 && turn % cadence !== 0) continue
    if (x - lastX < MIN_TURN_GAP_PX) continue
    result.push({ turn, x, label: `R${turn}` })
    lastX = x
  }
  return result
}

/** Fold spans narrower than `minSpan` into one mark per (lane, pixel bucket). */
export function foldSpans(spans: readonly PixelSpan[], width: number, minSpan = MIN_SPAN_PX): FoldedSpan[] {
  const w = Math.max(0, finite(width, 0))
  const unit = Math.max(Number.MIN_VALUE, finite(minSpan, MIN_SPAN_PX))
  if (w <= 0 || spans.length === 0) return []
  const project = (span: PixelSpan): FoldedSpan => ({
    left: Math.min(w, Math.max(0, span.left)),
    right: Math.min(w, Math.max(0, span.right)),
    lane: span.lane,
    error: !!span.error,
    selected: !!span.selected,
    dimmed: !!span.dimmed,
    outside: !!span.outside,
    index: span.index,
    count: 1,
  })
  if (spans.length * unit <= w) {
    return spans.filter(span => span.right > 0 && span.left < w).map(project)
  }
  const buckets = new Map<string, FoldedSpan>()
  for (const span of spans) {
    if (!Number.isFinite(span.left) || !Number.isFinite(span.right)) continue
    const left = Math.min(w, Math.max(0, span.left))
    const right = Math.min(w, Math.max(0, span.right))
    if (right <= 0 || left >= w) continue
    const bucket = Math.floor(left / unit)
    const key = `${span.lane}:${bucket}`
    const merged = buckets.get(key)
    if (merged) {
      merged.left = Math.min(merged.left, left)
      merged.right = Math.max(merged.right, right)
      merged.error = merged.error || !!span.error
      merged.selected = merged.selected || !!span.selected
      merged.dimmed = merged.dimmed && !!span.dimmed
      merged.outside = merged.outside && !!span.outside
      if ((merged.index < 0 && span.index >= 0) || (span.selected && span.index >= 0)) {
        merged.index = span.index
      }
      merged.count += 1
    } else {
      buckets.set(key, {
        left,
        right,
        lane: span.lane,
        error: !!span.error,
        selected: !!span.selected,
        dimmed: !!span.dimmed,
        outside: !!span.outside,
        index: span.index,
        count: 1,
      })
    }
  }
  return [...buckets.entries()]
    .sort((a, b) => {
      const [laneA, bucketA] = a[0].split(':').map(Number)
      const [laneB, bucketB] = b[0].split(':').map(Number)
      return laneA - laneB || bucketA - bucketB
    })
    .map(([, span]) => span)
}

export interface AxisScrollbar {
  /** Left edge of the thumb, 0..1 of the track. */
  thumbStart: number
  /** Width of the thumb, 0..1 of the track. */
  thumbWidth: number
  /** False when the whole domain already fits. */
  scrollable: boolean
}

/** Horizontal viewport scrollbar geometry, shared by all trajectory surfaces. */
export function axisScrollbar(options: {
  domainStart: number
  domainEnd: number
  viewStart: number
  viewSpan: number
}): AxisScrollbar {
  const { domainStart, domainEnd, viewStart, viewSpan } = options
  const total = Math.max(1e-9, domainEnd - domainStart)
  if (!Number.isFinite(total) || !Number.isFinite(viewStart) || !Number.isFinite(viewSpan)) {
    return { thumbStart: 0, thumbWidth: 1, scrollable: false }
  }
  const span = Math.min(Math.max(viewSpan, 0), total)
  const start = Math.min(Math.max(viewStart, domainStart), domainEnd - span)
  const thumbWidth = Math.min(Math.max(span / total, 0), 1)
  const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi)
  return {
    thumbStart: clamp((start - domainStart) / total, 0, 1 - thumbWidth),
    thumbWidth,
    scrollable: thumbWidth < 0.999,
  }
}

/** Map a pointer fraction on the track back to a clamped viewport start. */
export function scrollbarViewStart(
  domainStart: number,
  domainEnd: number,
  viewSpan: number,
  pointerFraction: number,
): number {
  const total = Math.max(1e-9, domainEnd - domainStart)
  if (!Number.isFinite(total) || !Number.isFinite(pointerFraction) || !Number.isFinite(viewSpan)) {
    return domainStart
  }
  const span = Math.min(Math.max(viewSpan, 0), total)
  const maxFraction = Math.max(0, 1 - span / total)
  const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi)
  return domainStart + clamp(pointerFraction, 0, maxFraction) * total
}

/** One idle gap collapsed by {@link IdleWarp}. */
export interface IdleGap {
  start: number
  end: number
  /** Time already removed before this gap started. */
  removedBefore: number
  /** Width the gap keeps after compression. */
  kept: number
}

export function gapSize(gap: IdleGap): number {
  return Math.max(0, gap.end - gap.start)
}

export function gapDropped(gap: IdleGap): number {
  return Math.max(0, gapSize(gap) - gap.kept)
}

/**
 * Piecewise-linear time warp that collapses long pauses between activity.
 * A thread can sit idle for hours between two rounds; painting wall-clock time
 * then reduces every span to a sliver.  The warp keeps covered intervals at
 * their real width and clamps every uncovered gap to a bounded width.
 */
export class IdleWarp {
  readonly gaps: IdleGap[]
  readonly base: number
  readonly realEnd: number

  constructor(gaps: IdleGap[], base: number, realEnd: number) {
    this.gaps = gaps
    this.base = base
    this.realEnd = realEnd
  }

  get isEmpty(): boolean {
    return this.gaps.length === 0
  }

  get dropped(): number {
    return this.gaps.reduce((sum, gap) => sum + gapDropped(gap), 0)
  }

  /** Compressed coordinate of a real timestamp. */
  map(time: number): number {
    if (this.gaps.length === 0 || !Number.isFinite(time)) return time
    let removed = 0
    for (const gap of this.gaps) {
      if (time >= gap.end) {
        removed = gap.removedBefore + gapSize(gap) - gap.kept
        continue
      }
      if (time > gap.start) {
        return gap.start - gap.removedBefore + Math.min(time - gap.start, gap.kept)
      }
      break
    }
    return time - removed
  }

  /** Real timestamp behind a compressed coordinate (idle start inside a gap). */
  unmap(value: number): number {
    if (this.gaps.length === 0 || !Number.isFinite(value)) return value
    let removed = 0
    for (const gap of this.gaps) {
      const start = gap.start - gap.removedBefore
      const end = start + gap.kept
      if (value >= end) {
        removed = gap.removedBefore + gapSize(gap) - gap.kept
        continue
      }
      if (value > start) return gap.start
      break
    }
    return value + removed
  }
}

/** One gap may keep at most this much, so a single enormous pause cannot
 * dominate an otherwise busy axis. */
export const IDLE_GAP_MAX_MS = 20000
/** All collapsed pauses together keep at most this fraction of covered time. */
export const IDLE_GAP_BUDGET = 0.01

/**
 * Build a warp from covered intervals (spans, buckets, round anchors).
 * Overlapping intervals merge first so fragments tiling the same activity
 * never create fake gaps.
 */
export function idleWarp(intervals: ReadonlyArray<readonly [number, number]>): IdleWarp {
  const ranges = (intervals || [])
    .filter(([start, end]) => Number.isFinite(start) && Number.isFinite(end))
    .map(([start, end]) => [start, Math.max(start, end)] as [number, number])
    .sort((a, b) => a[0] - b[0] || a[1] - b[1])
  if (ranges.length < 2) {
    const only = ranges[0]
    return new IdleWarp([], only ? only[0] : 0, only ? only[1] : 0)
  }
  const merged: [number, number][] = []
  for (const [start, end] of ranges) {
    const last = merged[merged.length - 1]
    if (last && start <= last[1]) last[1] = Math.max(last[1], end)
    else merged.push([start, end])
  }
  const base = merged[0][0]
  const realEnd = merged[merged.length - 1][1]
  if (merged.length < 2) return new IdleWarp([], base, realEnd)
  const active = merged.reduce((sum, [start, end]) => sum + (end - start), 0)
  const idle = merged
    .slice(1)
    .reduce((sum, [start], i) => sum + Math.max(0, start - merged[i][1]), 0)
  if (!(active > 0) || !(idle > 0)) return new IdleWarp([], base, realEnd)
  // Long pauses collapse to a hairline: the kept idle total is at most 1% of
  // covered time and every gap keeps a width proportional to its real length,
  // so the ruler reads as continuous activity while round boundaries stay
  // findable.
  const scale = Math.min((active * IDLE_GAP_BUDGET) / idle, 1)
  const gaps: IdleGap[] = []
  let removed = 0
  for (let i = 1; i < merged.length; i += 1) {
    const start = merged[i - 1][1]
    const end = merged[i][0]
    if (end <= start) continue
    const kept = Math.min((end - start) * scale, IDLE_GAP_MAX_MS)
    gaps.push({ start, end, removedBefore: removed, kept })
    removed += end - start - kept
  }
  return new IdleWarp(gaps, base, realEnd)
}

/**
 * Build the idle warp for one timeline: covered intervals are clipped at turn
 * anchors so an aggregate bucket cannot bridge a pause across rounds.
 */
export function timelineWarp(
  intervals: ReadonlyArray<readonly [number, number]>,
  anchors: readonly number[] = [],
): IdleWarp {
  const cuts = [...anchors].filter(Number.isFinite).sort((a, b) => a - b)
  const clipped: [number, number][] = []
  for (const pair of intervals || []) {
    const [rawStart, rawEnd] = pair
    if (!Number.isFinite(rawStart) || !Number.isFinite(rawEnd)) continue
    let lo = rawStart
    const end = Math.max(rawStart, rawEnd)
    for (const cut of cuts) {
      if (cut > lo && cut < end) {
        clipped.push([lo, cut])
        lo = cut
      }
    }
    clipped.push([lo, end])
  }
  return idleWarp(clipped)
}

/**
 * Initial viewport fraction for a dense thread.  Fitting thousands of events
 * into one track paints solid ribbons, so the first view keeps a bounded
 * number of events on screen (~4px each); the viewport scrollbar still covers
 * the whole thread.
 */
export function densityViewWidth(totalEvents: number, axisWidthPx: number): number {
  const width = Number.isFinite(axisWidthPx) ? Math.max(axisWidthPx, 1) : 1
  const target = Math.max(width / 4, 50)
  const total = Number(totalEvents)
  if (!Number.isFinite(total) || total <= target) return 1
  return Math.min(Math.max(target / total, 0.002), 1)
}
