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
        v-if="detailRootTurn"
        class="tt-toggle tt-overview"
        type="button"
        :title="t('messenger.trajectory.backToOverview')"
        @click="backToOverview"
      >
        {{ t('messenger.trajectory.backToOverview') }}
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
          :placeholder="'搜索完整线程'"
          spellcheck="false"
        >
      </div>
      <button
        class="tt-toggle tt-export"
        type="button"
        :disabled="exporting"
        :title="t('messenger.trajectory.export')"
        :aria-label="t('messenger.trajectory.export')"
        @click="exportLog"
      >
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <path d="M8 2.5v7m0 0 2.75-2.75M8 9.5 5.25 6.75" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round" />
          <path d="M3 11v1.75a.75.75 0 0 0 .75.75h8.5a.75.75 0 0 0 .75-.75V11" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
        <span>{{ t('messenger.trajectory.export') }}</span>
      </button>
    </header>

    <div v-if="searchQuery" class="tt-search-results">
      <span v-if="searchLoading">搜索中…</span><span v-if="searchError">搜索失败，请重试</span>
      <button v-for="hit in searchHits" :key="`${hit.body.body_id}:${hit.cursor}`" @click="openSearchHit(hit)">{{ hit.snippet }}</button>
      <span v-if="searchMore">已返回前 20 个命中，请缩小关键词范围</span>
      <pre v-if="searchText">{{ searchText }}</pre>
    </div>
    <div
      v-if="timeline"
      class="tt-timeline"
      :class="{ 'tt-panning': panning, 'tt-dragging': dragging }"
    >
      <div class="tt-plot">
        <div class="tt-plot-corner" aria-hidden="true" />
        <div class="tt-axis">
          <span
            v-for="tick in timelineTicks"
            :key="tick.value"
            class="tt-axis-tick"
            :style="{ left: `${tick.x}px` }"
          >{{ tick.label }}</span>
        </div>
        <div class="tt-lane-labels" aria-hidden="true">
          <span class="tt-lane-label">{{ t('messenger.trajectory.laneInput') }}</span>
          <span class="tt-lane-label">{{ t('messenger.trajectory.laneModel') }}</span>
          <span class="tt-lane-label">{{ t('messenger.trajectory.laneTool') }}</span>
        </div>
        <div
          ref="trackEl"
          class="tt-track"
          @wheel.prevent="onTimelineWheel"
          @contextmenu.prevent
          @pointerdown="onTimelinePointerDown"
          @pointermove="onTimelinePointerMove"
          @pointerup="onTimelinePointerUp"
          @pointerleave="onTimelineLeave"
          @dblclick="clearSelection"
        >
          <span
            v-for="span in projectedOverviewSpans"
            :key="span.key"
            class="tt-span tt-span-overview"
            :class="spanClasses(span)"
            :style="spanStyle(span)"
            :title="overviewSpanTitle(span)"
          />
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
        <div class="tt-truncated-hint tt-truncated-hint-top" role="status" aria-live="polite">
          <span>{{ olderLoading ? '正在读取记录…' : historyError ? '读取暂时失败，继续滚动可重试' : truncated ? '向上滚动查看更早记录' : '已到最早记录' }}</span>
        </div>
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
                :data-record-id="row.record.cell.recordId"
                @click="selectIndex(row.record.cell.index, { fromLedger: true })"
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
                      <button
                        v-if="row.record.cell.overviewOnly && row.record.turnStart"
                        class="tt-overview-open"
                        type="button"
                        @click.stop="openOverviewTurn(row.record.turn)"
                      >{{ t('messenger.trajectory.openCompleteTurn') }}</button>
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
        <div v-if="loading || loadError || records.length === 0" class="tt-empty">
          {{ loading
            ? t('common.loading')
            : loadError
              ? t('messenger.trajectory.loadFailed')
              : t('messenger.trajectory.empty') }}
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
            <span v-if="detailLoading" role="status">加载详情中…</span>
            <button v-if="detailError" @click="loadRecordDetail(selectedCell.recordId)">详情读取失败，重试</button>
            <pre v-if="(details[selectedCell.recordId] as any)?.raw">{{ (details[selectedCell.recordId] as any)?.raw }}</pre>
            <ThreadBodyViewport v-if="(details[selectedCell.recordId] as any)?.content_body" :key="selectedCell.recordId" :session-id="String(route.query.session ?? '')" :body="(details[selectedCell.recordId] as any).content_body" preview="正文按页读取" :cache-key="`trajectory:${selectedCell.recordId}`" />
            <button v-if="(details[selectedCell.recordId] as any)?.has_more" :disabled="detailLoading" @click="nextDetail">继续读取详情</button>
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
                <div v-if="selectedCell.thinkingDetail" class="tt-overview-section">
                  <button class="tt-section-title" type="button" @click="toggleSection('thinking')">
                    {{ t('messenger.trajectory.thinking') }}
                    <span class="tt-section-chevron" :class="{ 'tt-section-chevron-open': openSections.thinking }">›</span>
                  </button>
                  <div v-if="openSections.thinking" class="tt-section-scroll">
                    <div class="tt-markdown-payload" v-html="thinkingMarkdown" />
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
import { computed, markRaw, nextTick, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { useI18n } from '@/i18n'
import { searchThreadView, getThreadViewBootstrap, getThreadViewTrajectoryPage, getThreadViewBody } from '@/api/chat'
import ThreadBodyViewport from '@/components/chat/ThreadBodyViewport.vue'
import { exportSingleSessionLog } from '@/utils/sessionLogExport'
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
// 时间线 span 的 DOM 上限：百万级记录下若不限量会为每个 span 生成节点 + 4 个
// 事件监听器；这里在可视窗口内做等步长采样，保证 DOM 节点数有界。

const route = useRoute()
const router = useRouter()
const { t } = useI18n()

const modelT: Translate = (key, params) => t(`messenger.${key}`, params as never)

const loading = ref(true)
const olderLoading = ref(false)
const loadError = ref(false)
// W1-B：服务端只返回尾窗（truncated=true 表示更早轮次被截断）。这是有界读取的
// 正常结果，不是错误，也不应被当成空态。
const truncated = ref(false)
// 线程快照可能达百万级条目，用 shallowRef 只跟踪引用本身，避免 Vue 把整份
// 快照转成深层响应式代理（会引发 CPU/内存爆炸）；写入的大对象以 markRaw 包裹。
const rawTurns = shallowRef<unknown[]>([])
const displayPage=shallowRef<any>(null)
const historyError = ref(false)
const summaryRows = shallowRef<any[]>([])
const hasNewer = ref(false)
const TRAJECTORY_WINDOW_ROWS = 600
let retryHistoryAt = 0
const detailRootTurn=ref<string|null>(null)
const details=shallowRef<Record<string,unknown>>({})
const detailRefById=new Map<string,any>()
const turnRootByRound=new Map<number,string>()

// 大线程日志的快照可能非常庞大（压测会话可达百万级条目），单次全量拉取
// 会长时间占用主线程并可能触发默认 30s 超时。这里给一个显式上界并在
// 卸载时中止在途请求，避免视图长时间空转或卸载后回调写入已销毁的状态。
const TRAJECTORY_REQUEST_TIMEOUT_MS = 3000
let loadAbort: AbortController | null = null

const showDuration = ref(true)
const showTime = ref(false)
const showTurns = ref(false)
const showCalls = ref(false)
const searchQuery = ref('')

const searchHits=shallowRef<any[]>([]),searchLoading=ref(false),searchError=ref(false),searchMore=ref(false),searchText=ref('')
let searchAbort:AbortController|null=null,searchTimer:ReturnType<typeof setTimeout>|null=null
watch(searchQuery,()=>{searchAbort?.abort();if(searchTimer)clearTimeout(searchTimer);searchHits.value=[];searchText.value='';
 if(!searchQuery.value.trim())return
 searchTimer=setTimeout(async()=>{const controller=new AbortController();searchAbort=controller;searchLoading.value=true;searchError.value=false
  try{const response=await searchThreadView(String(route.query.session??''),searchQuery.value.slice(0,128),{signal:controller.signal});if(!controller.signal.aborted){searchHits.value=response.data.data.hits;searchMore.value=response.data.data.has_more}}
  catch{if(!controller.signal.aborted)searchError.value=true}finally{if(searchAbort===controller)searchLoading.value=false}
 },250)
})
async function openSearchHit(hit:any){searchAbort?.abort();const controller=new AbortController();searchAbort=controller
 try{const r=await getThreadViewBody(String(route.query.session??''),hit.body.body_id,{revision:hit.body.revision,cursor:hit.cursor},{signal:controller.signal});if(!controller.signal.aborted)searchText.value=r.data.data.chunks.join('')}
 catch{if(!controller.signal.aborted)searchError.value=true}
}

const selectedIndex = ref<number | null>(null)
const activeTab = ref('')
const detailsWidth = ref<number | null>(null)
const openSections = reactive({ input: true, output: true, thinking: true })
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
const filteredRecords = computed(() => allRecords.value)
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

type OverviewSpan = TimelineSpan & { count: number; aggregate: true }

const overviewTimeline = computed(() => {
  const page = displayPage.value
  const tiles = Array.isArray(page?.tiles) ? page.tiles : []
  if (tiles.length === 0) return null
  const level = Number.isFinite(Number(page?.tile_level)) ? Number(page.tile_level) : 0
  const bucketWidth = 128 * (8 ** level)
  const detailLaneCounts = [0, 0, 0]
  for (const span of timeline.value?.spans ?? []) detailLaneCounts[span.lane] = (detailLaneCounts[span.lane] ?? 0) + 1
  const detailTotal = detailLaneCounts.reduce((sum, value) => sum + value, 0)
  const ratios = detailTotal > 0
    ? detailLaneCounts.map(value => value / detailTotal)
    : [0.2, 0.5, 0.3]
  const spans: OverviewSpan[] = []
  let start = Infinity
  let end = -Infinity
  for (const tile of tiles) {
    const bucket = Number(tile?.bucket)
    const total = Math.max(0, Number(tile?.count) || 0)
    if (!Number.isFinite(bucket) || total <= 0) continue
    const explicit = [Number(tile?.input_count) || 0, Number(tile?.model_count) || 0, Number(tile?.tool_count) || 0]
    const explicitTotal = explicit.reduce((sum, value) => sum + value, 0)
    const laneCounts = explicitTotal > 0
      ? explicit
      : [
          Math.round(total * ratios[0]),
          Math.round(total * ratios[1]),
          Math.max(0, total - Math.round(total * ratios[0]) - Math.round(total * ratios[1])),
        ]
    const rawStart = timelineMode.value === 'sequence'
      ? bucket * bucketWidth
      : Number(tile?.start) * 1000
    const rawEnd = timelineMode.value === 'sequence'
      ? (bucket + 1) * bucketWidth
      : Math.max(rawStart, Number(tile?.end) * 1000)
    if (!Number.isFinite(rawStart) || !Number.isFinite(rawEnd)) continue
    start = Math.min(start, rawStart)
    end = Math.max(end, rawEnd)
    for (let lane = 0; lane < 3; lane += 1) {
      if (laneCounts[lane] <= 0) continue
      spans.push({
        start: rawStart,
        end: Math.max(rawEnd, rawStart + (timelineMode.value === 'sequence' ? 1 : 0.001)),
        index: -(spans.length + 1),
        isError: Number(tile?.failed) > 0,
        kind: lane === 0 ? 'context' : lane === 1 ? 'message' : 'tool',
        label: `${laneCounts[lane]} events`,
        lane,
        ttftFraction: null,
        count: laneCounts[lane],
        aggregate: true,
      })
    }
  }
  return spans.length > 0 ? { start, end: Math.max(end, start + 1), spans, turnBoundaries: [] } : null
})

const domain = computed(() => {
  const model = overviewTimeline.value ?? timeline.value
  if (!model) return { start: 0, end: 1 }
  return { start: model.start, end: Math.max(model.end, model.start + 1) }
})

watch([timeline, overviewTimeline], ([detailModel, overviewModel]) => {
  const model = overviewModel ?? detailModel
  if (!model) return
  viewStart.value = model.start
  viewSpan.value = Math.max(1, model.end - model.start)
})

const projectedOverviewSpans = computed(() => {
  const model = overviewTimeline.value
  if (!model) return []
  const width = trackWidth.value || 600
  const span = Math.max(1e-9, viewSpan.value)
  return model.spans.map((modelSpan, i) => {
    const left = ((modelSpan.start - viewStart.value) / span) * width
    const pw = ((modelSpan.end - modelSpan.start) / span) * width
    return {
      key: `overview-${modelSpan.index}-${i}`,
      model: modelSpan,
      left,
      width: Math.max(1, pw),
      dimmed: false,
      outside: false,
    }
  }).filter(item => item.left + item.width > 0 && item.left < width)
})

/** 选中变化后，若对应 span 不在时间线视口内则自动平移视口（对齐参考实现）。 */
watch(selectedIndex, (index) => {
  if (index === null) return
  const model = timeline.value
  if (!model) return
  const span = model.spans.find(entry => entry.index === index)
  if (!span) return
  const viewEnd = viewStart.value + viewSpan.value
  if (span.start >= viewStart.value && span.end <= viewEnd) return
  const nextStart = span.end > viewEnd ? span.end - viewSpan.value : span.start
  const clamped = clampViewport(nextStart, viewSpan.value)
  viewStart.value = clamped.start
  viewSpan.value = clamped.span
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
  if (!selection.value || dragging.value) return null
  const {start,end}=selection.value
  return new Set((timeline.value?.spans??[]).filter(s=>s.start<=end && s.end>=start).map(s=>s.index))
})

const projectedSpans = computed(() => {
  const model = timeline.value
  if (!model) return []
  const width = trackWidth.value || 600
  const span = Math.max(1e-9, viewSpan.value)
  const matched = searchMatched.value
  const projected = model.spans
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
  return projected
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

async function load(cursor: number | null = null, direction: 'older' | 'newer' | null = null): Promise<boolean> {
  const sessionId = String(route.query.session ?? '').trim()
  if (!sessionId) { loading.value = false; loadError.value = true; return false }
  loadAbort?.abort()
  const controller = new AbortController()
  loadAbort = controller
  if (direction) olderLoading.value = true
  else { loading.value = true; historyError.value = false; retryHistoryAt = 0 }
  loadError.value = false
  let timedOut = false
  const deadline = setTimeout(() => { timedOut = true; controller.abort() }, TRAJECTORY_REQUEST_TIMEOUT_MS)
  try {
    const params = { root_turn_id: detailRootTurn.value,
      ...(cursor === null ? {} : { [direction === 'newer' ? 'after' : 'before']: cursor }),
      limit: detailRootTurn.value ? 100 : 80 }
    const response = !direction && !detailRootTurn.value
      ? await getThreadViewBootstrap(sessionId, { kind: 'trajectory' }, { signal: controller.signal })
      : await getThreadViewTrajectoryPage(sessionId, params, { signal: controller.signal })
    if (controller.signal.aborted || loadAbort !== controller) return false
    const page = response.data.data.page
    if (page.degraded || !['ready', 'source'].includes(page.projection_state)) throw new Error('Display temporarily unavailable')
    if (direction && displayPage.value?.epoch !== page.epoch) return await load()
    const ledger = ledgerEl.value
    const previousTop = ledger?.scrollTop ?? 0
    const selectedId = selectedCell.value?.recordId
    const anchorNode = ledger && Array.from(ledger.querySelectorAll<HTMLElement>('[data-record-id]'))
      .find(node => node.getBoundingClientRect().bottom > ledger.getBoundingClientRect().top + 36)
    const anchorId = anchorNode?.dataset.recordId
    const anchorOffset = anchorNode && ledger ? anchorNode.getBoundingClientRect().top - ledger.getBoundingClientRect().top : 0
    const oldIndex = rows.value.findIndex(row => row.record.cell.recordId === anchorId)
    const oldOffset = oldIndex < 0 ? 0 : rowPrefix.value[oldIndex]
    const merged = new Map((direction ? summaryRows.value : []).map(row => [row.item_id, row]))
    for (const row of page.trajectory) merged.set(row.item_id, row)
    const ordered = [...merged.values()].sort((a, b) => a.created_seq - b.created_seq)
    const evicted = ordered.length > TRAJECTORY_WINDOW_ROWS
    summaryRows.value = markRaw(direction === 'older'
      ? ordered.slice(0, TRAJECTORY_WINDOW_ROWS) : ordered.slice(-TRAJECTORY_WINDOW_ROWS))
    if (direction === 'older') { truncated.value = page.has_more; hasNewer.value ||= evicted }
    else if (direction === 'newer') { hasNewer.value = Boolean(page.has_newer); truncated.value ||= evicted }
    else { truncated.value = page.has_more; hasNewer.value = false }
    displayPage.value = markRaw(page)
    rawTurns.value = summaryTurns(summaryRows.value)
    // Auxiliary state must have the same lifetime as the bounded ledger.
    const rounds = new Set(summaryRows.value.map(row => row.user_round))
    collapsedTurns.value = new Set([...collapsedTurns.value].filter(round => rounds.has(round)))
    const groups = new Set(allRecords.value.filter(record => record.group).map(record => requestIdentity(record.turn, record.group!)))
    collapsedAssistants.value = new Set([...collapsedAssistants.value].filter(key => groups.has(key)))
    selectedIndex.value = direction && selectedId
      ? allRecords.value.find(record => record.cell.recordId === selectedId)?.cell.index ?? null : null
    if (selectedIndex.value === null) { detailAbort?.abort(); details.value = {} }
    await nextTick()
    updateSizes()
    if (ledgerEl.value) {
      const nextIndex = rows.value.findIndex(row => row.record.cell.recordId === anchorId)
      ledgerEl.value.scrollTop = direction
        ? Math.max(0, previousTop + (nextIndex < 0 ? 0 : rowPrefix.value[nextIndex] - oldOffset))
        : ledgerEl.value.scrollHeight
      scrollTop.value = ledgerEl.value.scrollTop
      await nextTick()
      const node = Array.from(ledgerEl.value.querySelectorAll<HTMLElement>('[data-record-id]'))
        .find(row => row.dataset.recordId === anchorId)
      if (direction && node) ledgerEl.value.scrollTop += node.getBoundingClientRect().top - ledgerEl.value.getBoundingClientRect().top - anchorOffset
      scrollTop.value = ledgerEl.value.scrollTop
    }
    historyError.value = false
    return true
  } catch {
    if (loadAbort === controller && (!controller.signal.aborted || timedOut)) {
      if (direction) { historyError.value = true; retryHistoryAt = Date.now() + 2000 }
      else loadError.value = true
    }
  } finally {
    clearTimeout(deadline)
    if (loadAbort === controller) { loadAbort = null; loading.value = false; olderLoading.value = false }
  }
  return false
}
async function loadEdge(direction: 'older' | 'newer') {
  if (loading.value || olderLoading.value || Date.now() < retryHistoryAt ||
      !(direction === 'older' ? truncated.value : hasNewer.value)) return
  const cursor = (direction === 'older' ? summaryRows.value[0] : summaryRows.value.at(-1))?.created_seq
  if (cursor == null) return
  await load(cursor, direction)
}
function summaryTurns(rows:any[], resetMaps=true):unknown[] {
  if(resetMaps)detailRefById.clear()
  if(resetMaps)turnRootByRound.clear()
  const turns=new Map<string,any>()
  for(const row of rows){
    if (row.kind === 'source_overview') {
      turnRootByRound.set(Number(row.user_round),String(row.root_turn_id||''))
      const root = String(row.root_turn_id || '')
      if (root && !turns.has(root)) turns.set(root,{turn_id:root,user_turn_index:row.user_round,source_overview:true,items:[{
        item_id:row.item_id,item_index:0,kind:'assistant_message',status:'completed',created_time:row.started_at,updated_time:row.completed_at,
        payload:{content:t('messenger.trajectory.largeTurnOverview')},
      }]})
      continue
    }
    detailRefById.set(row.item_id,row.detail)
    turnRootByRound.set(Number(row.user_round),String(row.root_turn_id||''))
    let turn=turns.get(row.root_turn_id)
    if(!turn){turn={turn_id:row.root_turn_id,user_turn_index:row.user_round,items:[]};turns.set(row.root_turn_id,turn)}
    turn.items.push({item_id:row.item_id,item_index:row.item_index,kind:row.kind,status:row.status,created_time:row.started_at,updated_time:row.completed_at,
      overview_only:row.overview_only===true,
      payload:{content:row.summary,tool:row.tool_name,model_round:row.model_round,started_at:row.started_at,completed_at:row.completed_at}})
  }
  return markRaw([...turns.values()])
}
async function loadTurnDetail(round:number):Promise<void> {
  const root=turnRootByRound.get(round); if(!root || loading.value) return
  detailRootTurn.value=root
  await load()
}
function openOverviewTurn(round:number|null):void {
  if (round !== null) void loadTurnDetail(Number(round))
}
async function backToOverview(): Promise<void> {
  if (loading.value || !detailRootTurn.value) return
  detailRootTurn.value = null
  await load()
}
let detailAbort:AbortController|null=null
const detailLoading=ref(false),detailError=ref(false)
async function loadRecordDetail(id:string,cursor=0){
 const body=detailRefById.get(id);if(!body)return
 detailAbort?.abort();const controller=new AbortController();detailAbort=controller;detailLoading.value=true;detailError.value=false
 const session=String(route.query.session??'')
 try {
  const response=await getThreadViewBody(session,body.body_id,{revision:body.revision,cursor},{signal:controller.signal})
  if(controller.signal.aborted || detailAbort!==controller || selectedCell.value?.recordId!==id || String(route.query.session??'')!==session)return
  const page=response.data.data
  let value:any={raw:page.chunks.join(''),has_more:page.has_more,body,next:page.next_cursor}
  if(cursor===0 && !page.has_more){try{value={...value,...JSON.parse(value.raw)}}catch{}}
  details.value={[id]:markRaw(value)}
 }catch{if(!controller.signal.aborted && detailAbort===controller)detailError.value=true}
 finally{if(detailAbort===controller){detailAbort=null;detailLoading.value=false}}
}
async function nextDetail(){
 const cell=selectedCell.value;if(!cell)return
 const prior=details.value[cell.recordId] as any;if(!prior?.has_more || detailLoading.value)return
 await loadRecordDetail(cell.recordId,prior.next)
}


function goBack(): void {
  router.back()
}

/** 导出线程日志：与原线程日志对话框共用同一下载链路。 */
const exporting = ref(false)

async function exportLog(): Promise<void> {
  const sessionId = String(route.query.session ?? '').trim()
  if (!sessionId || exporting.value) return
  exporting.value = true
  try {
    await exportSingleSessionLog(sessionId)
    ElMessage.success(t('messenger.trajectory.exported'))
  } catch (error) {
    const detail = String((error as { message?: string })?.message || '').trim()
    ElMessage.error(t('messenger.trajectory.exportFailed', {
      message: detail || t('common.requestFailed'),
    }))
  } finally {
    exporting.value = false
  }
}

function onKeyDown(event: KeyboardEvent): void {
  if (event.key === 'Escape') {
    selection.value = null
    dragging.value = false
  }
}

function updateSizes(): void {
  if (ledgerEl.value) ledgerViewport.value = ledgerEl.value.clientHeight
}

// 时间线区块 v-if="timeline"：数据加载完成前轨道不存在，onMounted 测不到宽度。
// 通过 ref 侦测挂载并挂 ResizeObserver，轨道首次出现与尺寸变化时都重新测量，
// 否则 trackWidth 停留在默认 600，色块与刻度会挤在轨道左侧、右边留白。
let trackResizeObserver: ResizeObserver | null = null

watch(trackEl, (el) => {
  trackResizeObserver?.disconnect()
  trackResizeObserver = null
  if (!el) return
  trackWidth.value = el.clientWidth
  trackResizeObserver = new ResizeObserver(() => {
    trackWidth.value = el.clientWidth
  })
  trackResizeObserver.observe(el)
})

watch(()=>route.query.session,()=>{detailRootTurn.value=null;summaryRows.value=[];rawTurns.value=[];displayPage.value=null;details.value={};searchQuery.value='';searchAbort?.abort();detailAbort?.abort();void load()})

onMounted(() => {
  void load()
  window.addEventListener('keydown', onKeyDown)
  window.addEventListener('pointermove', onWindowPointerMove)
  window.addEventListener('pointerup', onWindowPointerUp)
  window.addEventListener('resize', updateSizes)
  updateSizes()
})

onBeforeUnmount(() => {
  loadAbort?.abort()
  detailAbort?.abort()
  searchAbort?.abort();if(searchTimer)clearTimeout(searchTimer)
  loadAbort = null
  window.removeEventListener('keydown', onKeyDown)
  window.removeEventListener('pointermove', onWindowPointerMove)
  window.removeEventListener('pointerup', onWindowPointerUp)
  window.removeEventListener('resize', updateSizes)
  trackResizeObserver?.disconnect()
  trackResizeObserver = null
  if (tooltipTimer !== null) window.clearTimeout(tooltipTimer)
  if (pointerRafId !== null) window.cancelAnimationFrame(pointerRafId)
  pointerRafId = null
})

/* ---------------------------------------------------------------- */
/* 台账                                                              */
/* ---------------------------------------------------------------- */

function onLedgerScroll(): void {
  if (!ledgerEl.value) return
  scrollTop.value = ledgerEl.value.scrollTop
  ledgerViewport.value = ledgerEl.value.clientHeight
  if (scrollTop.value <= 160) void loadEdge('older')
  else if (ledgerEl.value.scrollHeight - ledgerViewport.value - scrollTop.value <= 160) void loadEdge('newer')
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

// dsh 的请求小点全部固定在同一纵线上（base-left 12px，无逐轮偏移）。
const dotsByAssistantIndex = computed(() => {
  const map = new Map<number, RequestDot>()
  for (const request of requests.value) {
    map.set(request.assistantIndex, { request, left: 12 })
  }
  return map
})

function requestDotFor(record: TableRecord): RequestDot | null {
  return dotsByAssistantIndex.value.get(record.cell.index) ?? null
}

function onRowDoubleClick(record: TableRecord): void {
  if (record.collapsedSummary) return
  if (record.turnStart) {
    if (record.turn !== null && turnRootByRound.has(Number(record.turn))) { void loadTurnDetail(Number(record.turn)); return }
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

function selectIndex(index: number, options: { fromLedger?: boolean } = {}): void {
  selectedIndex.value = index
  const recordCell=findCell(index);if(recordCell)void loadRecordDetail(recordCell.recordId).catch(()=>{loadError.value=true})
  const cell = findCell(index)
  activeTab.value = defaultTabFor(cell)
  openSections.input = true
  openSections.output = true
  // 台账内点击不需要滚动；时间线与跳转链接的选中要把目标行带到可视区并高亮。
  if (!options.fromLedger) revealRecord(index)
}

/** 把选中的记录滚动到台账可视区中部（对齐参考实现的滚动定位）。 */
function revealRecord(index: number): void {
  const record = allRecords.value.find(entry => entry.cell.index === index)
  if (!record) return
  const assistantKey = requestIdentity(record.turn, record.group)
  let changed = false
  if (collapsedTurns.value.has(record.turn) && !record.turnStart) {
    const next = new Set(collapsedTurns.value)
    next.delete(record.turn)
    collapsedTurns.value = next
    changed = true
  }
  if (collapsedAssistants.value.has(assistantKey)) {
    const firstOfGroup = allRecords.value.find(entry =>
      entry.turn === record.turn && entry.group === record.group)
    if (firstOfGroup?.cell.index !== index) {
      const next = new Set(collapsedAssistants.value)
      next.delete(assistantKey)
      collapsedAssistants.value = next
      changed = true
    }
  }
  if (changed) {
    void nextTick(() => scrollLedgerToIndex(index))
  } else {
    scrollLedgerToIndex(index)
  }
}

function scrollLedgerToIndex(index: number): void {
  const position = rows.value.findIndex(row => row.record.cell.index === index)
  if (position === -1) return
  const top = rowPrefix.value[position] ?? 0
  const height = rows.value[position]?.height ?? 30
  const viewport = ledgerEl.value?.clientHeight ?? ledgerViewport.value
  if (viewport <= 0) return
  ledgerEl.value?.scrollTo({ top: Math.max(0, top + height / 2 - viewport / 2), behavior: 'smooth' })
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

const selectedCell = computed<TrajCell | null>(() => {
  const base=selectedIndex.value===null?null:findCell(selectedIndex.value)
  if(!base)return null
  const detail=details.value[base.recordId] as any
  if(!detail)return base
  const stringify=(v:unknown)=>typeof v==='string'?v:v===undefined?'':JSON.stringify(v,null,2)
  return {...base,inputDetail:stringify(detail.args??detail.content??detail.raw),outputDetail:stringify(detail.content??detail.raw),thinkingDetail:stringify(detail.reasoning??detail.reasoning_content),result:stringify(detail.result??detail.data??detail.raw),schemaDetail:stringify(detail.schema)}
})
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

function toggleSection(key: 'input' | 'output' | 'thinking'): void {
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
const thinkingMarkdown = computed(() => renderCellMarkdown(selectedCell.value?.thinkingDetail))

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
  schedulePointerMove(event, false)
}

function onWindowPointerUp(): void {
  resizeState = null
  flushPointerMove()
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
  schedulePointerMove(event, true)
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

/* 拖动/悬停的指针移动统一走帧级节流：同一帧内只处理最后一次事件，
   避免每次 pointermove 都全量重算时间线投影与 selection。 */
let pointerRafId: number | null = null
let pendingPointer: { event: PointerEvent; updateHover: boolean } | null = null

function schedulePointerMove(event: PointerEvent, updateHover: boolean): void {
  pendingPointer = { event, updateHover }
  if (pointerRafId !== null) return
  pointerRafId = window.requestAnimationFrame(() => {
    pointerRafId = null
    const pending = pendingPointer
    pendingPointer = null
    if (pending) applyPointerMove(pending.event, pending.updateHover)
  })
}

function applyPointerMove(event: PointerEvent, updateHover: boolean): void {
  if (updateHover && trackEl.value) {
    const rect = trackEl.value.getBoundingClientRect()
    hoverX.value = event.clientX >= rect.left && event.clientX <= rect.right
      ? event.clientX - rect.left
      : null
  }
  handleTimelineMove(event)
}

function flushPointerMove(): void {
  if (pointerRafId !== null) {
    window.cancelAnimationFrame(pointerRafId)
    pointerRafId = null
  }
  const pending = pendingPointer
  pendingPointer = null
  if (pending) applyPointerMove(pending.event, pending.updateHover)
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
  flushPointerMove()
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
    top: `${7 + span.model.lane * 14}px`,
  }
  if ((span.model as OverviewSpan).aggregate) {
    style.opacity = '0.32'
    style.pointerEvents = 'none'
  }
  if (span.model.kind === 'message' && span.model.ttftFraction !== null && span.width > 6) {
    const split = Math.round(span.width * span.model.ttftFraction)
    style.background = `linear-gradient(90deg, rgba(65,118,230,0.92) ${split}px, rgba(65,118,230,0.45) ${split}px)`
  }
  return style
}

function overviewSpanTitle(span: { model: TimelineSpan }): string {
  const count = Number((span.model as OverviewSpan).count || 0)
  return count > 0 ? `${count} 条记录（聚合概览）` : '聚合概览'
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

<style scoped>
</style>
