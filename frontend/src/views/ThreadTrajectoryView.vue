<template>
  <div class="tt-root">
    <header class="tt-toolbar">
      <button
        class="tt-icon-button"
        type="button"
        :title="t('messenger.trajectory.back')"
        :aria-label="t('messenger.trajectory.back')"
        @click="goBack"
      >
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path d="M10 3.5 5.5 8l4.5 4.5" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </button>
      <button
        class="tt-toggle"
        type="button"
        :class="{ 'tt-toggle-active': showDuration }"
        :aria-pressed="showDuration"
        @click="showDuration = !showDuration"
      >
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <circle cx="8" cy="8" r="5.25" fill="none" stroke="currentColor" stroke-width="1.25" />
          <path d="M8 4.75V8l2.25 1.5" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" />
        </svg>
        <span>{{ t('messenger.trajectory.duration') }}</span>
      </button>
      <button
        class="tt-toggle"
        type="button"
        :class="{ 'tt-toggle-active': showTurns }"
        :aria-pressed="showTurns"
        @click="showTurns = !showTurns"
      >
        <span class="tt-toggle-glyph">⊞</span>
        <span>{{ t('messenger.trajectory.turns') }}</span>
      </button>
      <button
        class="tt-toggle"
        type="button"
        :class="{ 'tt-toggle-active': showCalls }"
        :aria-pressed="showCalls"
        @click="showCalls = !showCalls"
      >
        <span class="tt-toggle-glyph">⊟</span>
        <span>{{ t('messenger.trajectory.calls') }}</span>
      </button>
      <div class="tt-toolbar-spacer" />
      <div class="tt-search">
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <circle cx="7" cy="7" r="4.4" fill="none" stroke="currentColor" stroke-width="1.25" />
          <path d="m10.4 10.4 3 3" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" />
        </svg>
        <input
          v-model="searchQuery"
          class="tt-search-input"
          type="text"
          :placeholder="t('messenger.trajectory.searchTrajectory')"
          spellcheck="false"
        >
      </div>
    </header>

    <div
      v-if="timeline"
      ref="timelineEl"
      class="tt-timeline"
      :class="{ 'tt-panning': panning, 'tt-dragging': dragging }"
      @wheel.prevent="onTimelineWheel"
      @contextmenu.prevent
      @pointerdown="onTimelinePointerDown"
      @pointermove="onTimelinePointerMove"
      @pointerup="onTimelinePointerUp"
      @pointerleave="onTimelineLeave"
      @dblclick="clearSelection"
    >
      <div class="tt-axis">
        <span
          v-for="tick in timelineTicks"
          :key="tick.value"
          class="tt-axis-tick"
          :style="{ left: `${tick.x}px` }"
        >{{ tick.label }}</span>
      </div>
      <div ref="trackEl" class="tt-track">
        <span class="tt-lane-label tt-lane-0">{{ t('messenger.trajectory.laneInput') }}</span>
        <span class="tt-lane-label tt-lane-1">{{ t('messenger.trajectory.laneModel') }}</span>
        <span class="tt-lane-label tt-lane-2">{{ t('messenger.trajectory.laneTool') }}</span>
        <span
          v-for="boundary in projectedBoundaries"
          :key="`b${boundary.turn}`"
          class="tt-turn-boundary"
          :style="{ left: `${boundary.x}px` }"
        />
        <span v-if="hoverX !== null" class="tt-hoverline" :style="{ left: `${hoverX}px` }" />
        <span
          v-for="span in projectedSpans"
          :key="span.key"
          class="tt-span"
          :class="spanClasses(span)"
          :style="spanStyle(span)"
          @pointerdown.stop
          @pointerenter="onSpanEnter(span, $event)"
          @pointerleave="onSpanLeave"
          @click.stop="selectIndex(span.model.index)"
        />
        <span
          v-if="selection"
          class="tt-selection"
          :style="selectionStyle"
        >
          <span class="tt-selection-edge tt-selection-edge-start" />
          <span class="tt-selection-edge tt-selection-edge-end" />
        </span>
      </div>
      <div
        v-if="tooltip.visible"
        class="tt-tooltip"
        :style="{ left: `${tooltip.x}px`, top: `${tooltip.y}px` }"
      >
        <div v-for="(line, i) in tooltip.lines" :key="i" class="tt-tooltip-line">{{ line }}</div>
      </div>
    </div>

    <div class="tt-body" :class="{ 'tt-has-details': selectedCell }">
      <div
        ref="ledgerEl"
        class="tt-ledger"
        @scroll.passive="onLedgerScroll"
        @pointerdown.self="clearSelection"
      >
        <div class="tt-ledger-canvas" :style="{ height: `${ledgerHeight}px` }">
          <table
            class="tt-table"
            :style="{ transform: `translateY(${windowTopPx}px)` }"
          >
            <colgroup>
              <col class="tt-col-event">
              <col>
            </colgroup>
            <tbody>
              <tr
                v-for="row in windowRows"
                :key="row.key"
                class="tt-row"
                :class="rowClasses(row.record)"
                :data-timeline-focus="focusAttr(row.record)"
                :data-record-index="row.record.cell.index"
                @click="selectIndex(row.record.cell.index)"
                @dblclick.stop="onRowDoubleClick(row.record)"
              >
                <td class="tt-event-cell">
                  <button
                    v-if="requestDotFor(row.record)"
                    class="tt-request-dot"
                    type="button"
                    :style="{ left: `${requestDotFor(row.record)!.left}px` }"
                    :title="t('messenger.trajectory.requestLabel', { request: requestDotFor(row.record)!.request.number })"
                    @click.stop="selectIndex(requestDotFor(row.record)!.request.assistantIndex)"
                  >
                    <span class="tt-request-dot-label">
                      {{ t('messenger.trajectory.requestLabel', { request: requestDotFor(row.record)!.request.number }) }}
                    </span>
                  </button>
                  <span v-if="row.record.turnStart && row.record.turn !== null" class="tt-turn-label">
                    {{ t('messenger.trajectory.turnLabel', { turn: row.record.turn }) }}
                  </span>
                </td>
                <td class="tt-content-cell">
                  <template v-if="row.record.collapsedSummary">
                    <div class="tt-collapsed-summary" :class="`tt-collapsed-${row.record.collapsedSummaryKind}`">
                      {{ row.record.collapsedSummary }}
                    </div>
                  </template>
                  <template v-else>
                    <div class="tt-kind-slot">
                      <span class="tt-kind-tag" :class="`tt-kind-${row.record.cell.kind}`">
                        {{ kindLabel(row.record.cell.kind) }}
                      </span>
                    </div>
                    <div class="tt-record-content">
                      <template v-if="row.record.cell.kind === 'tool' || row.record.cell.kind === 'subtool'">
                        <span class="tt-tool-name">{{ row.record.cell.text }}</span>
                        <span v-if="argsPreview(row.record.cell)" class="tt-tool-args">{{ argsPreview(row.record.cell) }}</span>
                        <span v-if="row.record.cell.resultPreviewMarkdown" class="tt-tool-result">
                          <span class="tt-tool-result-arrow">→</span>{{ row.record.cell.resultPreviewMarkdown }}
                        </span>
                        <span v-if="row.record.cell.isError" class="tt-state-chip tt-state-error">
                          {{ t('messenger.trajectory.stateFailed') }}
                        </span>
                      </template>
                      <template v-else-if="row.record.cell.toolCallOnly">
                        <span class="tt-record-empty">{{ t('messenger.trajectory.toolCallsOnly') }}</span>
                      </template>
                      <template v-else-if="row.record.cell.text">
                        <span class="tt-record-text">{{ row.record.cell.text }}</span>
                      </template>
                      <template v-else>
                        <span class="tt-record-empty">{{ t('messenger.trajectory.noContent') }}</span>
                      </template>
                    </div>
                  </template>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div v-if="records.length === 0" class="tt-empty">
          {{ loadError ? t('messenger.trajectory.loadFailed') : t('messenger.trajectory.empty') }}
        </div>
      </div>

      <template v-if="selectedCell && selectedRecord">
        <div class="tt-resize-handle" @pointerdown="startResize" @dblclick="detailsWidth = null" />
        <aside class="tt-details" :style="detailsStyle">
          <div class="tt-details-header">
            <span class="tt-kind-tag" :class="`tt-kind-${selectedCell.kind}`">
              {{ kindLabel(selectedCell.kind) }}
            </span>
            <span class="tt-details-location">{{ detailsLocation }}</span>
            <button
              class="tt-details-close"
              type="button"
              :title="t('messenger.trajectory.closeDetails')"
              @click="clearSelection"
            >
              <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
                <path d="m4 4 8 8m0-8-8 8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
              </svg>
            </button>
          </div>
          <div class="tt-detail-tabs">
            <button
              v-for="tab in detailTabs"
              :key="tab"
              class="tt-detail-tab"
              :class="{ 'tt-detail-tab-active': activeTab === tab }"
              type="button"
              @click="activeTab = tab"
            >{{ tab }}</button>
          </div>
          <div class="tt-detail-body">
            <!-- 概述 -->
            <div v-if="activeTab === overviewTab" class="tt-detail-body-summary">
              <dl class="tt-overview">
                <div class="tt-overview-row">
                  <dt>{{ t('messenger.trajectory.status') }}</dt>
                  <dd>{{ statusLabel(selectedCell) }}</dd>
                </div>
                <div v-if="hierarchyLinks.length > 0" class="tt-overview-row">
                  <dt>{{ t('messenger.trajectory.hierarchy') }}</dt>
                  <dd>
                    <button
                      v-for="link in hierarchyLinks"
                      :key="link.label"
                      class="tt-link"
                      type="button"
                      @click="link.action"
                    >{{ link.label }} ›</button>
                  </dd>
                </div>
                <div v-if="selectedCell.toolName" class="tt-overview-row">
                  <dt>{{ t('messenger.trajectory.kindTool') }}</dt>
                  <dd class="tt-mono">{{ selectedCell.toolName }}</dd>
                </div>
                <template v-if="selectedCell.usage">
                  <div class="tt-overview-row">
                    <dt>{{ t('messenger.trajectory.token') }}</dt>
                    <dd>{{ formatTokens(selectedCell.usage.output) }}</dd>
                  </div>
                  <div class="tt-overview-row">
                    <dt>{{ t('messenger.trajectory.reasoning') }}</dt>
                    <dd>{{ formatTokens(selectedCell.usage.think) }}</dd>
                  </div>
                </template>
                <div v-else-if="selectedCell.kind === 'message'" class="tt-overview-row">
                  <dt>{{ t('messenger.trajectory.token') }}</dt>
                  <dd>{{ t('messenger.trajectory.usageNotReported') }}</dd>
                </div>
                <div v-if="selectedCell.kind !== 'message'" class="tt-overview-row">
                  <dt>{{ t('messenger.trajectory.duration') }}</dt>
                  <dd>{{ durationValue(selectedCell.timeSeconds) }}</dd>
                </div>
                <div v-for="row in timingRows" :key="row.label" class="tt-overview-row">
                  <dt>{{ row.label }}</dt>
                  <dd :class="{ 'tt-dd-clickable': row.toggleable }" @click="row.toggleable ? toggleTimestampMode() : undefined">
                    {{ row.value }}
                  </dd>
                </div>
              </dl>
              <template v-if="selectedCell.kind === 'message'">
                <div v-if="selectedCell.inputDetail" class="tt-overview-section">
                  <button class="tt-section-title" type="button" @click="toggleSection('input')">
                    {{ t('messenger.trajectory.input') }}
                    <span class="tt-section-chevron" :class="{ 'tt-section-chevron-open': openSections.input }">›</span>
                  </button>
                  <div v-if="openSections.input" class="tt-section-scroll">
                    <div class="tt-markdown-payload" v-html="inputMarkdown" />
                  </div>
                </div>
                <div v-if="selectedCell.outputDetail" class="tt-overview-section">
                  <button class="tt-section-title" type="button" @click="toggleSection('output')">
                    {{ t('messenger.trajectory.output') }}
                    <span class="tt-section-chevron" :class="{ 'tt-section-chevron-open': openSections.output }">›</span>
                  </button>
                  <div v-if="openSections.output" class="tt-section-scroll">
                    <div class="tt-markdown-payload" v-html="outputMarkdown" />
                  </div>
                </div>
              </template>
            </div>
            <!-- 预览 -->
            <div v-else-if="activeTab === previewTab" class="tt-markdown-preview">
              <div class="tt-markdown-payload" v-html="previewMarkdown" />
            </div>
            <!-- 原始内容 -->
            <div v-else-if="activeTab === rawTab" class="tt-payload-stack">
              <pre v-if="selectedCell.thinkingDetail" class="tt-payload-pre"><code>{{ selectedCell.thinkingDetail }}</code></pre>
              <pre v-if="selectedCell.inputDetail" class="tt-payload-pre"><code>{{ selectedCell.inputDetail }}</code></pre>
              <div v-if="!selectedCell.inputDetail && !selectedCell.thinkingDetail" class="tt-no-payload">
                {{ t('messenger.trajectory.noContent') }}
              </div>
            </div>
            <!-- 参数 -->
            <div v-else-if="activeTab === paramsTab" class="tt-payload-stack">
              <TrajectoryJsonTree
                v-if="selectedArgs"
                :data="selectedArgs"
                :collapsed-string-lines="12"
              />
              <pre v-else-if="selectedCell.inputDetail" class="tt-payload-pre"><code>{{ selectedCell.inputDetail }}</code></pre>
              <div v-else class="tt-no-payload">{{ t('messenger.trajectory.paramUnavailable') }}</div>
            </div>
            <!-- 结果 -->
            <div v-else-if="activeTab === resultTab" class="tt-payload-stack">
              <TrajectoryJsonTree
                v-if="selectedResult"
                :data="selectedResult"
                :collapsed-string-lines="12"
              />
              <pre v-else-if="selectedCell.result" class="tt-payload-pre"><code>{{ selectedCell.result }}</code></pre>
              <div v-else class="tt-no-payload">{{ t('messenger.trajectory.noOutput') }}</div>
            </div>
            <!-- Schema -->
            <div v-else-if="activeTab === schemaTab" class="tt-payload-stack">
              <pre v-if="selectedCell.schemaDetail" class="tt-payload-pre"><code>{{ selectedCell.schemaDetail }}</code></pre>
              <div v-else class="tt-no-payload">{{ t('messenger.trajectory.schemaUnavailable') }}</div>
            </div>
            <!-- 计时 -->
            <div v-else class="tt-detail-body-summary">
              <dl class="tt-overview">
                <div v-for="row in fullTimingRows" :key="row.label" class="tt-overview-row">
                  <dt>{{ row.label }}</dt>
                  <dd :class="{ 'tt-dd-clickable': row.toggleable }" @click="row.toggleable ? toggleTimestampMode() : undefined">
                    {{ row.value }}
                  </dd>
                </div>
              </dl>
            </div>
          </div>
        </aside>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from '@/i18n'
import { getThreadLogSnapshot } from '@/api/chat'
import { renderMarkdown } from '@/utils/markdown'
import TrajectoryJsonTree from './trajectory/TrajectoryJsonTree.vue'
import {
  buildRequestNumbers,
  buildTrajectoryLayout,
  collapseTurnRecords,
  deriveTimeline,
  filterRecords,
  flattenRecords,
  formatClock,
  formatDurationMillis,
  formatElapsedSeconds,
  formatStartedAt,
  groupVirtualRows,
  requestIdentity,
  timelineFocusIndexes,
} from './trajectory/trajectoryModel'
import type {
  TableRecord,
  TimelineSpan,
  TrajCell,
  TrajKind,
  TrajRequestNumber,
  Translate,
} from './trajectory/trajectoryModel'

const OVERSCAN_PX = 360
const VIRTUAL_THRESHOLD = 100
const MIN_VIEWPORT_MS = 20
const MIN_VIEWPORT_OPS = 4
const EDGE_PAN_FRACTION = 0.08
const TOOLTIP_DELAY_MS = 500

const route = useRoute()
const router = useRouter()
const { t } = useI18n()

const modelT: Translate = (key, params) => t(`messenger.${key}`, params as never)

const loading = ref(true)
const loadError = ref(false)
const rawTurns = ref<unknown[]>([])

const showDuration = ref(true)
const showTime = ref(false)
const showTurns = ref(false)
const showCalls = ref(false)
const searchQuery = ref('')

const selectedIndex = ref<number | null>(null)
const activeTab = ref('')
const detailsWidth = ref<number | null>(null)
const openSections = reactive({ input: true, output: true })
const timestampMode = ref<'local' | 'unix'>('local')

const collapsedTurns = ref(new Set<number | null>())
const collapsedAssistants = ref(new Set<string>())

const trackEl = ref<HTMLElement | null>(null)
const ledgerEl = ref<HTMLElement | null>(null)
const scrollTop = ref(0)
const ledgerViewport = ref(360)
const trackWidth = ref(600)

const viewStart = ref(0)
const viewSpan = ref(0)
const hoverX = ref<number | null>(null)
const panning = ref(false)
const dragging = ref(false)
const selection = ref<{ start: number; end: number } | null>(null)
const tooltip = reactive({ visible: false, x: 0, y: 0, lines: [] as string[] })

let dragState: {
  kind: 'select' | 'pan'
  anchorX: number
  anchorValue: number
  moved: boolean
  panStart: number
} | null = null
let tooltipTimer: number | null = null
let resizeState: { startX: number; startWidth: number } | null = null

const timelineMode = computed(() => {
  if (showDuration.value && showTime.value) return 'actual' as const
  if (showDuration.value) return 'duration' as const
  if (showTime.value) return 'time' as const
  return 'sequence' as const
})

const turns = computed(() => buildTrajectoryLayout(rawTurns.value, modelT))
const requests = computed(() => buildRequestNumbers(turns.value))
const allRecords = computed(() => flattenRecords(turns.value))
const filteredRecords = computed(() => filterRecords(allRecords.value, searchQuery.value))
const records = computed(() => collapseTurnRecords(
  filteredRecords.value,
  collapsedTurns.value,
  collapsedAssistants.value,
  modelT,
))
const layout = computed(() => groupVirtualRows(records.value))
const rows = computed(() => layout.value.rows)
const ledgerHeight = computed(() =>
  rows.value.reduce((total, row) => total + row.height, 0))
const virtual = computed(() => ledgerHeight.value > VIRTUAL_THRESHOLD)

const rowPrefix = computed<number[]>(() => {
  const prefix: number[] = [0]
  for (const row of rows.value) prefix.push(prefix[prefix.length - 1] + row.height)
  return prefix
})

const windowRange = computed(() => {
  if (!virtual.value) return { start: 0, end: rows.value.length }
  const top = Math.max(0, scrollTop.value - OVERSCAN_PX)
  const bottom = scrollTop.value + ledgerViewport.value + OVERSCAN_PX
  let start = 0
  while (start < rows.value.length && rowPrefix.value[start + 1]! <= top) start += 1
  let end = start
  while (end < rows.value.length && rowPrefix.value[end]! < bottom) end += 1
  return { start, end }
})

const windowRows = computed(() =>
  rows.value.slice(windowRange.value.start, windowRange.value.end))

const windowTopPx = computed(() => {
  if (!virtual.value) return 0
  return rowPrefix.value[windowRange.value.start] ?? 0
})

const timeline = computed(() => deriveTimeline(turns.value, timelineMode.value))

const domain = computed(() => {
  const model = timeline.value
  if (!model) return { start: 0, end: 1 }
  return { start: model.start, end: Math.max(model.end, model.start + 1) }
})

watch(timeline, (model) => {
  if (!model) return
  viewStart.value = model.start
  viewSpan.value = Math.max(1, model.end - model.start)
})

const searchNeedle = computed(() => searchQuery.value.trim().toLowerCase())
const searchMatched = computed<ReadonlySet<number>>(() => {
  const needle = searchNeedle.value
  if (!needle) return new Set()
  const matched = new Set<number>()
  for (const record of allRecords.value) {
    if (recordNeedleText(record).includes(needle)) matched.add(record.cell.index)
  }
  return matched
})

function recordNeedleText(record: TableRecord): string {
  return [
    record.cell.text,
    record.cell.toolName ?? '',
    record.cell.inputDetail ?? '',
    record.cell.outputDetail ?? '',
    record.cell.result ?? '',
  ].join('\n').toLowerCase()
}

const focusSet = computed<ReadonlySet<number> | null>(() => {
  if (!selection.value) return null
  return timelineFocusIndexes(turns.value, selection.value, timelineMode.value)
})

const projectedSpans = computed(() => {
  const model = timeline.value
  if (!model) return []
  const width = trackWidth.value || 600
  const span = Math.max(1e-9, viewSpan.value)
  const matched = searchMatched.value
  return model.spans
    .map((modelSpan, i) => {
      const left = ((modelSpan.start - viewStart.value) / span) * width
      const pw = ((modelSpan.end - modelSpan.start) / span) * width
      const gap = Math.min(pw * 0.08, 1)
      return {
        key: `${modelSpan.index}-${i}`,
        model: modelSpan,
        left: left + gap,
        width: Math.max(2, pw - gap * 2),
        dimmed: searchNeedle.value ? !matched.has(modelSpan.index) : false,
        outside: focusSet.value ? !focusSet.value.has(modelSpan.index) : false,
      }
    })
    .filter(span => span.left + span.width > 0 && span.left < width)
})

const projectedBoundaries = computed(() => {
  const model = timeline.value
  if (!model || !showTurns.value) return []
  const width = trackWidth.value || 600
  const span = Math.max(1e-9, viewSpan.value)
  return model.turnBoundaries
    .map(boundary => ({ turn: boundary.turn, x: ((boundary.time - viewStart.value) / span) * width }))
    .filter(boundary => boundary.x >= 0 && boundary.x <= width)
})

const timelineTicks = computed(() => {
  const width = trackWidth.value || 600
  if (width <= 0) return []
  const span = Math.max(1e-9, viewSpan.value)
  const targetCount = Math.max(2, Math.floor(width / 120))
  const raw = span / targetCount
  const magnitude = 10 ** Math.floor(Math.log10(raw))
  const residual = raw / magnitude
  const step = (residual >= 5 ? 5 : residual >= 2 ? 2 : 1) * magnitude
  const timed = timelineMode.value !== 'sequence'
  const ticks: { value: number; x: number; label: string }[] = []
  const first = Math.ceil(viewStart.value / step) * step
  for (let value = first; value <= viewStart.value + span; value += step) {
    const x = ((value - viewStart.value) / span) * width
    ticks.push({
      value,
      x,
      label: timed ? formatAxisDuration(value - domain.value.start) : String(Math.round(value)),
    })
  }
  return ticks
})

function pad2(value: number): string {
  return String(value).padStart(2, '0')
}

function formatAxisDuration(ms: number): string {
  const abs = Math.abs(ms)
  if (abs < 1000) return `${Math.round(abs)} ms`
  if (abs < 60_000) return `${(abs / 1000).toFixed(abs < 10_000 ? 1 : 0)} s`
  if (abs < 3_600_000) {
    const minutes = Math.floor(abs / 60_000)
    const seconds = Math.round((abs % 60_000) / 1000)
    return seconds > 0 ? `${minutes}m ${pad2(seconds)}s` : `${minutes}m`
  }
  const hours = Math.floor(abs / 3_600_000)
  const minutes = Math.round((abs % 3_600_000) / 60_000)
  return `${hours}h ${pad2(minutes)}m`
}

/* ---------------------------------------------------------------- */
/* 数据加载                                                          */
/* ---------------------------------------------------------------- */

async function load(): Promise<void> {
  const sessionId = String(route.query.session ?? '').trim()
  if (!sessionId) {
    loading.value = false
    loadError.value = true
    return
  }
  loading.value = true
  loadError.value = false
  try {
    const snapshot = await getThreadLogSnapshot(sessionId)
    const payload = snapshot?.data
    rawTurns.value = Array.isArray(payload?.turns) ? payload.turns : []
  } catch {
    loadError.value = true
    rawTurns.value = []
  } finally {
    loading.value = false
  }
}

function goBack(): void {
  router.back()
}

function onKeyDown(event: KeyboardEvent): void {
  if (event.key === 'Escape') {
    selection.value = null
    dragging.value = false
  }
}

function updateSizes(): void {
  if (trackEl.value) trackWidth.value = trackEl.value.clientWidth
  if (ledgerEl.value) ledgerViewport.value = ledgerEl.value.clientHeight
}

onMounted(() => {
  void load()
  window.addEventListener('keydown', onKeyDown)
  window.addEventListener('pointermove', onWindowPointerMove)
  window.addEventListener('pointerup', onWindowPointerUp)
  window.addEventListener('resize', updateSizes)
  updateSizes()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeyDown)
  window.removeEventListener('pointermove', onWindowPointerMove)
  window.removeEventListener('pointerup', onWindowPointerUp)
  window.removeEventListener('resize', updateSizes)
  if (tooltipTimer !== null) window.clearTimeout(tooltipTimer)
})

/* ---------------------------------------------------------------- */
/* 台账                                                              */
/* ---------------------------------------------------------------- */

function onLedgerScroll(): void {
  if (!ledgerEl.value) return
  scrollTop.value = ledgerEl.value.scrollTop
  ledgerViewport.value = ledgerEl.value.clientHeight
}

function rowClasses(record: TableRecord): Record<string, boolean> {
  return {
    'tt-row-selected': selectedCell.value?.index === record.cell.index,
    'tt-row-turn-start': record.turnStart,
    'tt-row-turn-end': record.turnEnd,
    'tt-row-summary': Boolean(record.collapsedSummary),
    [`tt-row-${record.cell.kind}`]: true,
    'tt-row-error': record.cell.isError === true,
  }
}

function focusAttr(record: TableRecord): string | undefined {
  if (!focusSet.value) return undefined
  return focusSet.value.has(record.cell.index) ? 'inside' : 'outside'
}

function kindLabel(kind: TrajKind): string {
  const labels: Record<TrajKind, string> = {
    system: t('messenger.trajectory.kindSystem'),
    user: t('messenger.trajectory.kindUser'),
    context: t('messenger.trajectory.kindContext'),
    compacted: t('messenger.trajectory.kindCompacted'),
    message: t('messenger.trajectory.kindAssistant'),
    tool: t('messenger.trajectory.kindTool'),
    subtool: t('messenger.trajectory.kindSubtool'),
  }
  return labels[kind]
}

function argsPreview(cell: TrajCell): string {
  const raw = cell.inputDetail
  if (!raw) return ''
  const single = raw.replace(/\s+/g, ' ').trim()
  return single.length > 160 ? `${single.slice(0, 160)}…` : single
}

interface RequestDot { request: TrajRequestNumber; left: number }

const dotsByAssistantIndex = computed(() => {
  const map = new Map<number, RequestDot>()
  const perTurn = new Map<number | null, number>()
  for (const request of requests.value) {
    const count = perTurn.get(request.turn) ?? 0
    perTurn.set(request.turn, count + 1)
    map.set(request.assistantIndex, { request, left: 12 + (count % 4) * 8 })
  }
  return map
})

function requestDotFor(record: TableRecord): RequestDot | null {
  return dotsByAssistantIndex.value.get(record.cell.index) ?? null
}

function onRowDoubleClick(record: TableRecord): void {
  if (record.collapsedSummary) return
  if (record.turnStart) {
    const next = new Set(collapsedTurns.value)
    if (next.has(record.turn)) next.delete(record.turn)
    else next.add(record.turn)
    collapsedTurns.value = next
    return
  }
  if (record.groupStart && record.group) {
    const key = requestIdentity(record.turn, record.group)
    const next = new Set(collapsedAssistants.value)
    if (next.has(key)) next.delete(key)
    else next.add(key)
    collapsedAssistants.value = next
  }
}

watch([showTurns, showCalls], ([turnsOn, callsOn]) => {
  if (!turnsOn) collapsedTurns.value = new Set()
  if (!callsOn) collapsedAssistants.value = new Set()
})

/* ---------------------------------------------------------------- */
/* 选中与详情                                                        */
/* ---------------------------------------------------------------- */

function selectIndex(index: number): void {
  selectedIndex.value = index
  const cell = findCell(index)
  activeTab.value = defaultTabFor(cell)
  openSections.input = true
  openSections.output = true
}

function findCell(index: number): TrajCell | null {
  for (const turn of turns.value) {
    for (const group of turn.groups) {
      for (const cell of group.cells) {
        if (cell.index === index) return cell
      }
    }
  }
  return null
}

function findRecordOf(cell: TrajCell): TableRecord | null {
  return allRecords.value.find(record => record.cell.index === cell.index) ?? null
}

const selectedCell = computed<TrajCell | null>(() =>
  selectedIndex.value === null ? null : findCell(selectedIndex.value))
const selectedRecord = computed<TableRecord | null>(() => {
  const cell = selectedCell.value
  return cell ? findRecordOf(cell) : null
})

function requestIdentityForCell(cell: TrajCell): string {
  const record = findRecordOf(cell)
  return record ? requestIdentity(record.turn, record.group) : ''
}

const requestForCell = computed<TrajRequestNumber | null>(() => {
  const cell = selectedCell.value
  if (!cell) return null
  const direct = requests.value.find(request => request.assistantIndex === cell.index)
  if (direct) return direct
  if (cell.kind === 'tool' || cell.kind === 'subtool') {
    const identity = requestIdentityForCell(cell)
    return requests.value.find(request => request.identity === identity) ?? null
  }
  return null
})

const hierarchyLinks = computed(() => {
  const cell = selectedCell.value
  const links: { label: string; action: () => void }[] = []
  if (!cell) return links
  const request = requestForCell.value
  if ((cell.kind === 'tool' || cell.kind === 'subtool') && request) {
    links.push({
      label: t('messenger.trajectory.assistantMessage'),
      action: () => selectIndex(request.assistantIndex),
    })
  }
  if (request) {
    links.push({
      label: t('messenger.trajectory.requestLabel', { request: request.number }),
      action: () => selectIndex(request.assistantIndex),
    })
  }
  return links
})

function statusLabel(cell: TrajCell): string {
  if (cell.isError) return t('messenger.trajectory.stateFailed')
  if (cell.kind === 'message' || cell.kind === 'tool' || cell.kind === 'subtool') {
    return cell.metrics?.timingRecorded || cell.timeSeconds !== null
      ? t('messenger.trajectory.stateCompleted')
      : t('messenger.trajectory.stateWaiting')
  }
  return t('messenger.trajectory.stateCompleted')
}

interface TimingRow { label: string; value: string; toggleable?: boolean }

function startedAtValue(ms: number | null): string {
  if (ms === null) return t('messenger.trajectory.notRecorded')
  if (timestampMode.value === 'unix') return String(Math.round(ms))
  return formatStartedAt(ms) || t('messenger.trajectory.unavailable')
}

const timingRows = computed<TimingRow[]>(() => {
  const cell = selectedCell.value
  if (!cell) return []
  if (cell.kind === 'user' || cell.kind === 'context' || cell.kind === 'system') return []
  const metrics = cell.metrics
  const rowsOut: TimingRow[] = []
  if (cell.kind === 'message' && (!metrics || !metrics.timingRecorded)) {
    return [{ label: t('messenger.trajectory.timingSource'), value: t('messenger.trajectory.notRecorded') }]
  }
  if (cell.kind === 'message' && metrics) {
    rowsOut.push({
      label: t('messenger.trajectory.startedAt'),
      value: startedAtValue(metrics.stepStartTime),
      toggleable: true,
    })
    rowsOut.push({
      label: t('messenger.trajectory.totalDuration'),
      value: metrics.completedTime !== null && metrics.stepStartTime !== null
        ? formatDurationMillis(metrics.completedTime - metrics.stepStartTime, modelT)
        : t('messenger.trajectory.notRecorded'),
    })
    if (metrics.firstTokenTime !== null && metrics.stepStartTime !== null) {
      rowsOut.push({
        label: t('messenger.trajectory.firstToken'),
        value: formatDurationMillis(metrics.firstTokenTime - metrics.stepStartTime, modelT),
      })
    }
    const decode = decodeSeconds(cell)
    if (decode !== null) {
      rowsOut.push({
        label: t('messenger.trajectory.generation'),
        value: formatDurationMillis(decode * 1000, modelT),
      })
    }
    const throughput = decodeThroughput(cell)
    if (throughput !== null) {
      rowsOut.push({
        label: t('messenger.trajectory.throughput'),
        value: t('messenger.trajectory.tokensPerSecond', { value: throughput.toFixed(1) }),
      })
    }
  }
  if (cell.kind === 'tool' || cell.kind === 'subtool') {
    rowsOut.push({
      label: t('messenger.trajectory.startedAt'),
      value: startedAtValue(cell.startedAt),
      toggleable: true,
    })
    rowsOut.push({
      label: t('messenger.trajectory.duration'),
      value: durationValue(cell.timeSeconds),
    })
  }
  return rowsOut
})

const fullTimingRows = computed<TimingRow[]>(() => {
  const cell = selectedCell.value
  if (!cell) return []
  const rowsOut: TimingRow[] = [...timingRows.value]
  if (cell.kind === 'message') {
    rowsOut.push({
      label: t('messenger.trajectory.sessionTimestamp'),
      value: timestampMode.value === 'local'
        ? t('messenger.trajectory.showUnixTimestamp')
        : t('messenger.trajectory.showLocalTime'),
      toggleable: true,
    })
  }
  return rowsOut
})

/** 时长展示：未记录的时间戳不伪造 0 值。 */
function durationValue(seconds: number | null): string {
  return seconds === null
    ? t('messenger.trajectory.notRecorded')
    : formatElapsedSeconds(seconds, modelT)
}

function decodeSeconds(cell: TrajCell): number | null {
  const metrics = cell.metrics
  if (!metrics?.timingRecorded || metrics.firstTokenTime === null || metrics.completedTime === null) {
    return null
  }
  return Math.max(0, (metrics.completedTime - metrics.firstTokenTime) / 1000)
}

function decodeThroughput(cell: TrajCell): number | null {
  const decode = decodeSeconds(cell)
  const tokens = cell.usage?.output
  if (decode === null || decode <= 0 || tokens === undefined || tokens <= 0) return null
  return tokens / decode
}

function toggleTimestampMode(): void {
  timestampMode.value = timestampMode.value === 'local' ? 'unix' : 'local'
}

function toggleSection(key: 'input' | 'output'): void {
  openSections[key] = !openSections[key]
}

const overviewTab = computed(() => t('messenger.trajectory.tabOverview'))
const previewTab = computed(() => t('messenger.trajectory.tabPreview'))
const rawTab = computed(() => t('messenger.trajectory.tabRaw'))
const paramsTab = computed(() => t('messenger.trajectory.tabParams'))
const resultTab = computed(() => t('messenger.trajectory.tabResult'))
const schemaTab = computed(() => t('messenger.trajectory.tabSchema'))

const detailTabs = computed<string[]>(() => {
  const cell = selectedCell.value
  if (!cell) return []
  if (cell.kind === 'system') {
    return [overviewTab.value, t('messenger.trajectory.tabRaw')]
  }
  if (cell.kind === 'compacted') {
    return [overviewTab.value, t('messenger.trajectory.tabRawOutput')]
  }
  if (cell.kind === 'tool' || cell.kind === 'subtool') {
    return [
      overviewTab.value,
      paramsTab.value,
      resultTab.value,
      schemaTab.value,
      t('messenger.trajectory.tabTiming'),
    ]
  }
  return [overviewTab.value, previewTab.value, rawTab.value]
})

function defaultTabFor(cell: TrajCell | null): string {
  if (cell?.kind === 'tool' || cell?.kind === 'subtool') return paramsTab.value
  return overviewTab.value
}

watch(detailTabs, (tabs) => {
  if (!tabs.includes(activeTab.value)) activeTab.value = tabs[0] ?? ''
})

const detailsLocation = computed(() => {
  const record = selectedRecord.value
  const cell = selectedCell.value
  if (!record || !cell) return ''
  const turnPart = record.turn === null
    ? t('messenger.trajectory.betweenTurns')
    : t('messenger.trajectory.turnTitle', { turn: record.turn })
  const stepMatch = record.group ? /(\d+)/.exec(record.group) : null
  if (stepMatch) {
    return `${turnPart} · ${t('messenger.trajectory.stepLabel', { step: Number(stepMatch[1]) })}`
  }
  if (cell.kind === 'compacted') return `${turnPart} · ${t('messenger.trajectory.compaction')}`
  if (record.group) return `${turnPart} · ${record.group}`
  return turnPart
})

function safeParse(raw: string | undefined): unknown {
  if (!raw) return null
  try {
    return JSON.parse(raw)
  } catch {
    return null
  }
}

const selectedArgs = computed(() => safeParse(selectedCell.value?.inputDetail))
const selectedResult = computed(() => safeParse(selectedCell.value?.result))

function renderCellMarkdown(raw: string | undefined): string {
  if (!raw) return ''
  try {
    return renderMarkdown(raw)
  } catch {
    return ''
  }
}

const previewMarkdown = computed(() => renderCellMarkdown(selectedCell.value?.previewMarkdown))
const inputMarkdown = computed(() => renderCellMarkdown(selectedCell.value?.inputDetail))
const outputMarkdown = computed(() => renderCellMarkdown(selectedCell.value?.outputDetail))

const detailsStyle = computed(() =>
  detailsWidth.value === null ? undefined : { width: `${detailsWidth.value}px` })

function startResize(event: PointerEvent): void {
  event.preventDefault()
  const target = event.currentTarget as HTMLElement
  const aside = target.parentElement?.querySelector<HTMLElement>('.tt-details')
  if (!aside) return
  resizeState = {
    startX: event.clientX,
    startWidth: aside.getBoundingClientRect().width,
  }
  target.setPointerCapture(event.pointerId)
}

function onWindowPointerMove(event: PointerEvent): void {
  if (resizeState) {
    const delta = resizeState.startX - event.clientX
    const maxWidth = Math.max(320, window.innerWidth - 280)
    detailsWidth.value = Math.min(720, Math.max(320, Math.min(maxWidth, resizeState.startWidth + delta)))
  }
  handleTimelineMove(event)
}

function onWindowPointerUp(): void {
  resizeState = null
  finishTimelinePointer()
}

/* ---------------------------------------------------------------- */
/* 时间线交互                                                        */
/* ---------------------------------------------------------------- */

function clampViewport(start: number, span: number): { start: number; span: number } {
  const { start: domainStart, end: domainEnd } = domain.value
  const domainSpan = Math.max(1e-9, domainEnd - domainStart)
  const nextSpan = Math.min(Math.max(span, minSpan()), domainSpan)
  const maxStart = domainEnd - nextSpan
  return {
    start: Math.min(Math.max(start, domainStart), Math.max(domainStart, maxStart)),
    span: nextSpan,
  }
}

function minSpan(): number {
  return timelineMode.value === 'sequence' ? MIN_VIEWPORT_OPS : MIN_VIEWPORT_MS
}

function valueAt(clientX: number): number {
  const rect = trackEl.value?.getBoundingClientRect()
  if (!rect || rect.width <= 0) return viewStart.value
  const fraction = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width))
  return viewStart.value + fraction * viewSpan.value
}

function onTimelineWheel(event: WheelEvent): void {
  const anchor = valueAt(event.clientX)
  const factor = event.deltaY > 0 ? 1.25 : 0.8
  const nextSpan = viewSpan.value * factor
  const fraction = (anchor - viewStart.value) / Math.max(1e-9, viewSpan.value)
  const nextStart = anchor - fraction * nextSpan
  const clamped = clampViewport(nextStart, nextSpan)
  viewStart.value = clamped.start
  viewSpan.value = clamped.span
}

function onTimelinePointerDown(event: PointerEvent): void {
  if (!trackEl.value) return
  const value = valueAt(event.clientX)
  if (event.button === 2) {
    dragState = {
      kind: 'pan',
      anchorX: event.clientX,
      anchorValue: value,
      moved: false,
      panStart: viewStart.value,
    }
    panning.value = true
    return
  }
  if (event.button !== 0) return
  dragState = {
    kind: 'select',
    anchorX: event.clientX,
    anchorValue: value,
    moved: false,
    panStart: viewStart.value,
  }
  dragging.value = true
  selection.value = null
}

function onTimelinePointerMove(event: PointerEvent): void {
  if (trackEl.value) {
    const rect = trackEl.value.getBoundingClientRect()
    hoverX.value = event.clientX >= rect.left && event.clientX <= rect.right
      ? event.clientX - rect.left
      : null
  }
  handleTimelineMove(event)
}

function handleTimelineMove(event: PointerEvent): void {
  if (!dragState || !trackEl.value) return
  if (dragState.kind === 'pan') {
    const dx = event.clientX - dragState.anchorX
    if (Math.abs(dx) > 2) dragState.moved = true
    const rect = trackEl.value.getBoundingClientRect()
    const perPx = viewSpan.value / Math.max(1, rect.width)
    const clamped = clampViewport(dragState.panStart - dx * perPx, viewSpan.value)
    viewStart.value = clamped.start
    viewSpan.value = clamped.span
    return
  }
  const rect = trackEl.value.getBoundingClientRect()
  if (Math.abs(event.clientX - dragState.anchorX) >= 3) dragState.moved = true
  edgePan(event.clientX, rect)
  const anchor = valueAt(dragState.anchorX)
  const current = valueAt(event.clientX)
  selection.value = {
    start: Math.min(anchor, current),
    end: Math.max(anchor, current),
  }
}

function edgePan(clientX: number, rect: DOMRect): void {
  const edge = rect.width * EDGE_PAN_FRACTION
  let shift = 0
  if (clientX - rect.left < edge) shift = -viewSpan.value * 0.03
  else if (rect.right - clientX < edge) shift = viewSpan.value * 0.03
  if (shift !== 0) {
    const clamped = clampViewport(viewStart.value + shift, viewSpan.value)
    viewStart.value = clamped.start
    viewSpan.value = clamped.span
  }
}

function finishTimelinePointer(): void {
  if (!dragState) return
  const state = dragState
  dragState = null
  dragging.value = false
  panning.value = false
  if (state.kind !== 'select') return
  if (!state.moved) {
    // 视为点击：聚焦时间上最近的记录。
    const model = timeline.value
    if (model && model.spans.length > 0) {
      let best = model.spans[0]!
      let bestDistance = Number.POSITIVE_INFINITY
      for (const span of model.spans) {
        const center = (span.start + span.end) / 2
        const distance = Math.abs(center - state.anchorValue)
        if (distance < bestDistance) {
          bestDistance = distance
          best = span
        }
      }
      selection.value = null
      selectIndex(best.index)
    }
    return
  }
  if (selection.value && selection.value.end - selection.value.start < 1e-9) {
    selection.value = null
  }
}

function onTimelinePointerUp(): void {
  finishTimelinePointer()
}

function onTimelineLeave(): void {
  hoverX.value = null
}

function onSpanEnter(span: { model: TimelineSpan }, event: PointerEvent): void {
  if (tooltipTimer !== null) window.clearTimeout(tooltipTimer)
  const clientX = event.clientX
  const clientY = event.clientY
  tooltipTimer = window.setTimeout(() => {
    const cell = findCell(span.model.index)
    const lines: string[] = [kindLabel(span.model.kind)]
    if (span.model.label) {
      lines.push(span.model.label.length > 64 ? `${span.model.label.slice(0, 64)}…` : span.model.label)
    }
    if (cell) {
      if (cell.startedAt !== null) {
        const end = cell.startedAt + (cell.timeSeconds ?? 0) * 1000
        lines.push(`${formatClock(cell.startedAt)} → ${formatClock(end)}`)
      }
      if (cell.timeSeconds !== null && cell.timeSeconds > 0) {
        lines.push(t('messenger.trajectory.totalDurationValue', {
          duration: formatDurationMillis(cell.timeSeconds * 1000, modelT),
        }))
      }
      const decode = decodeSeconds(cell)
      const metrics = cell.metrics
      const ttft = metrics?.firstTokenTime != null && metrics.stepStartTime != null
        ? metrics.firstTokenTime - metrics.stepStartTime
        : null
      if (ttft !== null && decode !== null) {
        lines.push(t('messenger.trajectory.tooltipTtft', {
          ttft: formatDurationMillis(ttft, modelT),
          decoding: formatDurationMillis(decode * 1000, modelT),
        }))
      }
    }
    tooltip.lines = lines
    tooltip.x = clientX
    tooltip.y = clientY
    tooltip.visible = true
  }, TOOLTIP_DELAY_MS)
}

function onSpanLeave(): void {
  if (tooltipTimer !== null) window.clearTimeout(tooltipTimer)
  tooltip.visible = false
}

function clearSelection(): void {
  selection.value = null
  selectedIndex.value = null
  activeTab.value = ''
}

function spanClasses(span: { model: TimelineSpan; dimmed: boolean; outside: boolean }): Record<string, boolean> {
  return {
    [`tt-span-${span.model.kind}`]: true,
    'tt-span-error': span.model.isError,
    'tt-span-dimmed': span.dimmed,
    'tt-span-outside': span.outside,
  }
}

function spanStyle(span: { model: TimelineSpan; left: number; width: number }): Record<string, string> {
  const style: Record<string, string> = {
    left: `${span.left}px`,
    width: `${span.width}px`,
    top: `${span.model.lane * 14}px`,
  }
  if (span.model.kind === 'message' && span.model.ttftFraction !== null && span.width > 6) {
    const split = Math.round(span.width * span.model.ttftFraction)
    style.background = `linear-gradient(90deg, rgba(65,118,230,0.92) ${split}px, rgba(65,118,230,0.45) ${split}px)`
  }
  return style
}

const selectionStyle = computed(() => {
  if (!selection.value) return {}
  const width = trackWidth.value || 600
  const span = Math.max(1e-9, viewSpan.value)
  const left = ((selection.value.start - viewStart.value) / span) * width
  const right = ((selection.value.end - viewStart.value) / span) * width
  return {
    left: `${Math.max(0, left)}px`,
    width: `${Math.max(2, right - left)}px`,
  }
})

function formatTokens(value: number | undefined): string {
  if (value === undefined || !Number.isFinite(value)) return t('messenger.trajectory.unavailable')
  return t('messenger.trajectory.tokens', { value: Math.round(value).toLocaleString('en-US') })
}
</script>
