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
        :class="{ 'tt-toggle-active': showTokens }"
        :aria-pressed="showTokens"
        @click="showTokens = !showTokens"
      >
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <rect x="3" y="8" width="2.4" height="5" rx="0.6" fill="currentColor" />
          <rect x="7" y="5" width="2.4" height="8" rx="0.6" fill="currentColor" />
          <rect x="11" y="9.5" width="2.4" height="3.5" rx="0.6" fill="currentColor" />
        </svg>
        <span>{{ t('messenger.trajectory.tokenUsage') }}</span>
      </button>
      <button
        class="tt-toggle"
        type="button"
        :class="{ 'tt-toggle-active': showTurns }"
        :aria-pressed="showTurns"
        :title="t(showTurns ? 'messenger.trajectory.expandTurns' : 'messenger.trajectory.collapseTurns')"
        @click="toggleTurns"
      >
        <span class="tt-toggle-glyph">{{ showTurns ? '⊞' : '⊟' }}</span>
        <span>{{ t('messenger.trajectory.turns') }}</span>
      </button>
      <button
        class="tt-toggle"
        type="button"
        :class="{ 'tt-toggle-active': showCalls }"
        :aria-pressed="showCalls"
        :title="t(showCalls ? 'messenger.trajectory.expandCalls' : 'messenger.trajectory.collapseCalls')"
        @click="toggleCalls"
      >
        <span class="tt-toggle-glyph">{{ showCalls ? '⊞' : '⊟' }}</span>
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
          <template v-if="overviewTimeline">
            <span
              v-for="span in projectedOverviewSpans"
              :key="span.key"
              class="tt-span tt-span-overview"
              :class="spanClasses(span)"
              :style="spanStyle(span)"
              :title="overviewSpanTitle(span)"
              @pointerdown.stop
              @pointerenter="span.model.index >= 0 && onSpanEnter(span, $event)"
              @pointerleave="onSpanLeave"
              @click.stop="span.model.index >= 0 && selectIndex(span.model.index)"
            />
          </template>
          <span
            v-for="boundary in projectedBoundaries"
            :key="`b${boundary.turn}`"
            class="tt-turn-boundary"
            :style="{ left: `${boundary.x}px` }"
          />
          <span v-if="hoverX !== null" class="tt-hoverline" :style="{ left: `${hoverX}px` }" />
          <template v-if="!overviewTimeline">
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
          </template>
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

    <div v-if="showTokens && stepUsage.bars.length > 0" class="tt-tokenbars">
      <span class="tt-tokenbars-label">{{ t('messenger.trajectory.tokenUsage') }}</span>
      <div class="tt-tokenbars-track">
        <span
          v-for="bar in stepUsage.bars"
          :key="bar.requestNumber"
          class="tt-tokenbar"
          :class="tokenBarClasses(bar)"
          :style="{ height: tokenBarHeight(bar) }"
          @pointerenter="onTokenBarEnter(bar, $event)"
          @pointerleave="onSpanLeave"
          @click.stop="onTokenBarClick(bar)"
        />
      </div>
      <span class="tt-tokenbars-summary">{{
        stepUsage.reportedCount > 0
          ? t('messenger.trajectory.tokenTotal', { value: `${formatCompactTokens(stepUsage.totalSum)} tok` })
          : ''
      }}</span>
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
                :data-timeline-focus="row.record.collapsedSummary ? undefined : focusAttr(row.record)"
                :data-record-index="row.record.collapsedSummary ? undefined : row.record.cell.index"
                :data-record-id="row.record.collapsedSummary ? undefined : row.record.cell.recordId"
                :tabindex="0"
                :role="row.record.collapsedSummary ? 'button' : undefined"
                :aria-label="row.record.collapsedSummary ? row.record.collapsedSummary + ' · ' + t('messenger.trajectory.expand') : undefined"
                @keydown.enter.self.prevent="onRowClick(row.record)"
                @keydown.space.self.prevent="onRowClick(row.record)"
                @click="onRowClick(row.record)"
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
                    @dblclick.stop
                  >
                    <span class="tt-request-dot-label">
                      {{ t('messenger.trajectory.requestLabel', { request: requestDotFor(row.record)!.request.number }) }}
                    </span>
                  </button>
                  <button v-if="row.record.turnStart && row.record.turn !== null" class="tt-turn-label tt-turn-control"
                    type="button" :disabled="!collapsibleTurnKeys.has(row.record.turn)"
                    :aria-expanded="!collapsedTurns.has(row.record.turn)"
                    :title="t(collapsedTurns.has(row.record.turn) ? 'messenger.trajectory.expandTurn' : 'messenger.trajectory.collapseTurn')"
                    @click.stop="toggleTurn(row.record.turn)" @dblclick.stop>
                    <span aria-hidden="true">{{ collapsedTurns.has(row.record.turn) ? '▸' : '▾' }}</span>
                    {{ t('messenger.trajectory.turnLabel', { turn: row.record.turn }) }}
                  </button>
                </td>
                <td class="tt-content-cell">
                  <template v-if="row.record.collapsedSummary">
                    <div class="tt-collapsed-summary" :class="`tt-collapsed-${row.record.collapsedSummaryKind}`" :title="row.record.collapsedSummary + ' · ' + t('messenger.trajectory.expand')">
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
                      <span v-if="row.record.cell.isError && row.record.cell.kind !== 'tool' && row.record.cell.kind !== 'subtool'" class="tt-state-chip tt-state-error"
                        :title="row.record.cell.failure?.message">{{ statusLabel(row.record.cell) }}</span>
                      <span v-else-if="row.record.cell.retry?.willRetry && row.record.cell.status === 'running'" class="tt-state-chip">
                        {{ t('messenger.trajectory.stateRetrying') }}
                      </span>
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
              </template>
                <div v-else-if="selectedCell.kind === 'message'" class="tt-overview-row">
                  <dt>{{ t('messenger.trajectory.token') }}</dt>
                  <dd>{{ t('messenger.trajectory.usageNotReported') }}</dd>
                </div>
                <div v-for="row in timingRows" :key="row.label" class="tt-overview-row">
                  <dt>{{ row.label }}</dt>
                  <dd :class="{ 'tt-dd-clickable': row.toggleable }" @click="row.toggleable ? toggleTimestampMode() : undefined">
                    {{ row.value }}
                  </dd>
                </div>
                <div v-for="row in trajectoryDiagnosticRows(selectedCell, modelT)" :key="row.label" class="tt-overview-row tt-diagnostic-row">
                  <dt>{{ row.label }}</dt>
                  <dd>{{ row.value }}</dd>
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
              <template v-else-if="selectedCell.kind === 'compacted'">
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
            <div v-else-if="activeTab === rawTab || (selectedCell.kind === 'compacted' && activeTab === t('messenger.trajectory.tabRawOutput'))" class="tt-payload-stack">
              <pre v-if="selectedCell.kind === 'compacted' && selectedCell.outputDetail" class="tt-payload-pre"><code>{{ selectedCell.outputDetail }}</code></pre>
              <pre v-if="selectedCell.thinkingDetail" class="tt-payload-pre"><code>{{ selectedCell.thinkingDetail }}</code></pre>
              <pre v-if="selectedCell.inputDetail" class="tt-payload-pre"><code>{{ selectedCell.inputDetail }}</code></pre>
              <div v-if="!selectedCell.inputDetail && !selectedCell.thinkingDetail && !selectedCell.outputDetail" class="tt-no-payload">
                {{ t('messenger.trajectory.noContent') }}
              </div>
            </div>
            <!-- 参数 -->
            <div v-else-if="activeTab === paramsTab" class="tt-payload-stack">
              <div v-if="detailLoading" class="tt-no-payload">{{ t('common.loading') }}</div>
              <div v-else-if="detailError" class="tt-no-payload">
                {{ t('messenger.trajectory.detailLoadFailed') }}
                <button class="tt-link" type="button" @click="retrySelectedDetail">{{ t('messenger.trajectory.retryDetail') }}</button>
              </div>
              <template v-else>
              <TrajectoryJsonTree
                v-if="selectedArgs"
                :data="selectedArgs"
                :collapsed-string-lines="12"
              />
              <pre v-else-if="selectedCell.inputDetail" class="tt-payload-pre"><code>{{ selectedCell.inputDetail }}</code></pre>
              <div v-else class="tt-no-payload">{{ t('messenger.trajectory.paramUnavailable') }}</div>
              </template>
            </div>
            <!-- 结果 -->
            <div v-else-if="activeTab === resultTab" class="tt-payload-stack">
              <div v-if="detailLoading" class="tt-no-payload">{{ t('common.loading') }}</div>
              <div v-else-if="detailError" class="tt-no-payload">
                {{ t('messenger.trajectory.detailLoadFailed') }}
                <button class="tt-link" type="button" @click="retrySelectedDetail">{{ t('messenger.trajectory.retryDetail') }}</button>
              </div>
              <template v-else>
              <TrajectoryJsonTree
                v-if="selectedResult"
                :data="selectedResult"
                :collapsed-string-lines="12"
              />
              <pre v-else-if="selectedCell.result" class="tt-payload-pre"><code>{{ selectedCell.result }}</code></pre>
              <div v-else class="tt-no-payload">{{ t('messenger.trajectory.noOutput') }}</div>
              </template>
            </div>
            <!-- Schema -->
            <div v-else-if="activeTab === schemaTab" class="tt-payload-stack">
              <div v-if="detailLoading" class="tt-no-payload">{{ t('common.loading') }}</div>
              <div v-else-if="detailError" class="tt-no-payload">
                {{ t('messenger.trajectory.detailLoadFailed') }}
                <button class="tt-link" type="button" @click="retrySelectedDetail">{{ t('messenger.trajectory.retryDetail') }}</button>
              </div>
              <template v-else>
              <pre v-if="selectedCell.schemaDetail" class="tt-payload-pre"><code>{{ selectedCell.schemaDetail }}</code></pre>
              <div v-else class="tt-no-payload">{{ t('messenger.trajectory.schemaUnavailable') }}</div>
              </template>
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
import { searchThreadView, getThreadViewTrajectoryPage, getThreadViewBody } from '@/api/chat'
import { exportSingleSessionLog } from '@/utils/sessionLogExport'
import { renderMarkdown } from '@/utils/markdown'
import TrajectoryJsonTree from './trajectory/TrajectoryJsonTree.vue'
import {
  buildRequestNumbers,
  buildStepUsageBars,
  buildTrajectoryLayout,
  collapseTurnRecords,
  buildFoldGroups,
  trajectoryRecordId,
  deriveTimeline,
  extractUsage,
  filterRecords,
  flattenRecords,
  formatClock,
  formatDurationMillis,
  formatElapsedSeconds,
  formatStartedAt,
  groupVirtualRows,
  requestIdentity,
  timelineFocusIndexes,
  trajectoryFailure,
  trajectoryRetry,
  trajectoryDiagnosticRows,
  mergeTrajectoryPage,
  trajectoryPageTurns,
} from './trajectory/trajectoryModel'
import type {
  StepUsageBar,
  TableRecord,
  TimelineMode,
  TimelineModel,
  TimelineSpan,
  TrajCell,
  TrajKind,
  TrajRequestNumber,
  Translate,
  TrajectoryPageRow,
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
// 服务端只返回逐条记录尾窗（truncated=true 表示还有更早记录）。这是有界读取的
// 正常结果，不是错误，也不应被当成空态。
const truncated = ref(false)
// 线程快照可能达百万级条目，用 shallowRef 只跟踪引用本身，避免 Vue 把整份
// 快照转成深层响应式代理（会引发 CPU/内存爆炸）；写入的大对象以 markRaw 包裹。
const rawTurns = shallowRef<unknown[]>([])
const displayPage=shallowRef<any>(null)
const historyError = ref(false)
const trajectoryRows = shallowRef<TrajectoryPageRow[]>([])
const hasNewer = ref(false)
const beforeCursor = ref<number | null>(null)
const afterCursor = ref<number | null>(null)
const TRAJECTORY_WINDOW_ROWS = 600
const TRAJECTORY_PAGE_ROWS = 80
let retryHistoryAt = 0
const details=shallowRef<Record<string,unknown>>({})
const detailRefById=new Map<string,any>()
// Detail payloads are intentionally hydrated only for the selected row.  The
// trajectory page contains individual bounded previews, while args/schema/result live in
// the immutable per-item body referenced by `row.detail`.
const detailLoading = ref(false)
const detailError = ref(false)
const detailCache = new Map<string, { revision: number; value: unknown }>()
let detailAbort: AbortController | null = null
let detailRequestSerial = 0

// 大线程日志的快照可能非常庞大（压测会话可达百万级条目），单次全量拉取
// 会长时间占用主线程并可能触发默认 30s 超时。这里给一个显式上界并在
// 卸载时中止在途请求，避免视图长时间空转或卸载后回调写入已销毁的状态。
const TRAJECTORY_REQUEST_TIMEOUT_MS = 3000
const DETAIL_REQUEST_TIMEOUT_MS = 5000
const DETAIL_MAX_BYTES = 2 * 1024 * 1024
let loadAbort: AbortController | null = null

const showDuration = ref(true)
const showTime = ref(false)
const showTokens = ref(true)
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
const stepUsage = computed(() => buildStepUsageBars(requests.value))
const allRecords = computed(() => flattenRecords(turns.value))
const filteredRecords = computed(() => filterRecords(allRecords.value, searchQuery.value))
const foldGroups = computed(() => buildFoldGroups(allRecords.value))
const collapsibleTurnKeys = computed(() => new Set([...foldGroups.value.turns]
  .filter(([, turn]) => turn.hidden > 0).map(([turn]) => turn)))
const collapsibleAssistantKeys = computed(() => new Set(foldGroups.value.assistants.keys()))
const showTurns = computed(() => collapsibleTurnKeys.value.size > 0
  && [...collapsibleTurnKeys.value].every(turn => collapsedTurns.value.has(turn)))
const showCalls = computed(() => collapsibleAssistantKeys.value.size > 0
  && [...collapsibleAssistantKeys.value].every(key => collapsedAssistants.value.has(key)))
function toggleTurns(): void {
  collapsedTurns.value = showTurns.value ? new Set() : new Set(collapsibleTurnKeys.value)
}
function toggleCalls(): void {
  collapsedAssistants.value = showCalls.value ? new Set() : new Set(collapsibleAssistantKeys.value)
}
function toggleTurn(turn: number | null): void {
  if (turn === null || !collapsibleTurnKeys.value.has(turn)) return
  const next = new Set(collapsedTurns.value)
  if (next.has(turn)) next.delete(turn)
  else next.add(turn)
  collapsedTurns.value = next
}
const records = computed(() => searchQuery.value.trim() ? filteredRecords.value : collapseTurnRecords(
  filteredRecords.value, collapsedTurns.value, collapsedAssistants.value, modelT,
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

watch(rows, (nextRows, previousRows) => {
  const ledger = ledgerEl.value
  if (!ledger || loading.value || olderLoading.value || previousRows.length === 0) return
  const top = ledger.scrollTop
  let offset = 0
  let anchor = previousRows[0]!.record
  for (const row of previousRows) {
    anchor = row.record
    if (offset + row.height > top) break
    offset += row.height
  }
  let target = nextRows.findIndex(row => !row.record.collapsedSummary && row.record.cell.recordId === anchor.cell.recordId)
  if (target < 0) target = nextRows.findIndex(row => row.record.turn === anchor.turn)
  const nextTop = target < 0 ? 0 : rowPrefix.value[target]! + top - offset
  scrollTop.value = Math.max(0, Math.min(nextTop, ledgerHeight.value - ledgerViewport.value))
  void nextTick(() => { if (ledgerEl.value) ledgerEl.value.scrollTop = scrollTop.value })
})

const windowRange = computed(() => {
  if (!virtual.value) return { start: 0, end: rows.value.length }
  const viewportTop = Math.min(scrollTop.value, Math.max(0, ledgerHeight.value - ledgerViewport.value))
  const top = Math.max(0, viewportTop - OVERSCAN_PX)
  const bottom = viewportTop + ledgerViewport.value + OVERSCAN_PX
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

type OverviewSpan = TimelineSpan & { count: number; aggregate?: true; bucket?: number }
type TimelineRow = { created_seq?: unknown; kind?: unknown; status?: unknown; started_at?: unknown; completed_at?: unknown }

// The server returns at most 1024 aggregate buckets. Keep that bounded
// overview, but subdivide each bucket into a small number of deterministic
// slices so the overview retains the segmented feel of DSH.
const MAX_OVERVIEW_SPANS = 4096

function timestampMs(value: unknown): number | null {
  const numeric = Number(value)
  if (!Number.isFinite(numeric) || numeric <= 0) return null
  const milliseconds = Math.abs(numeric) < 1e11 ? numeric * 1000 : numeric
  return Number.isFinite(milliseconds) && milliseconds > 0 ? milliseconds : null
}

function overviewKind(kind: unknown): TrajKind {
  const value = String(kind || '')
  if (value === 'assistant_message') return 'message'
  if (value === 'tool_call' || value === 'tool_result') return 'tool'
  if (value === 'subagent_run') return 'subtool'
  if (value === 'compaction' || value === 'compacted') return 'compacted'
  if (value === 'system_message') return 'system'
  if (value === 'user_message') return 'user'
  return 'context'
}

function laneFor(kind: TrajKind): number {
  if (kind === 'tool' || kind === 'subtool') return 2
  if (kind === 'message' || kind === 'compacted') return 1
  return 0
}

/** A bounded timing-only stream uses the same idle-gap compression as the
 * detailed ledger, so paging older rows cannot change the initial overview. */
function serverOverviewTimeline(rows: unknown[], mode: TimelineMode, indexBySeq: ReadonlyMap<number, number>): TimelineModel | null {
  const raw = (Array.isArray(rows) ? rows : []).flatMap((row, index): TimelineSpan[] => {
    if (!row || typeof row !== 'object') return []
    const item = row as TimelineRow
    const start = mode === 'sequence' ? Number(item.created_seq) : timestampMs(item.started_at)
    if (start === null || !Number.isFinite(start)) return []
    const kind = overviewKind(item.kind)
    const completed = mode === 'sequence' ? null : timestampMs(item.completed_at)
    // Input/context rows are points even if their durable bookkeeping row
    // was updated at turn completion. Giving them that lifetime bridges idle
    // gaps and paints a misleading full-width input lane.
    const end = mode === 'sequence' ? start + 1
      : laneFor(kind) === 0 ? start : Math.max(start, completed ?? start)
    return [{ start, end, index: indexBySeq.get(Number(item.created_seq)) ?? -(index + 1), isError: ['failed', 'error', 'cancelled', 'interrupted', 'rejected'].includes(String(item.status || '')),
      kind, label: '', lane: laneFor(kind), ttftFraction: null }]
  })
  if (raw.length === 0) return null
  raw.sort((left, right) => left.start - right.start || left.end - right.end || left.index - right.index)
  const compressed = mode === 'duration'
  let removed = 0
  let covered: number | null = null
  const spans = raw.map(span => {
    if (compressed && covered !== null && span.start > covered) removed += span.start - covered
    covered = covered === null ? span.end : Math.max(covered, span.end)
    return { ...span, start: span.start - removed, end: (mode === 'sequence' || mode === 'duration' || mode === 'actual' ? span.end : span.start) - removed }
  })
  let start = Infinity; let end = -Infinity
  for (const span of spans) { start = Math.min(start, span.start); end = Math.max(end, span.end) }
  return Number.isFinite(start) && Number.isFinite(end)
    ? { start, end: Math.max(end, start + 1), spans, turnBoundaries: [] } : null
}

const overviewTimeline = computed(() => {
  const page = displayPage.value
  // A tail-only timeline is useful as a bounded fallback, but it must not be
  // presented as the full session domain: older spans would otherwise appear
  // to move when the user scrolls and loads them.
  const indexById = new Map(allRecords.value.map(record => [record.cell.recordId, record.cell.index]))
  const indexBySeq = new Map<number, number>()
  for (const row of trajectoryRows.value) {
    const index = indexById.get(String(row.item_id))
    if (index !== undefined) indexBySeq.set(Number(row.created_seq), index)
  }
  const serverTimeline = page?.timeline_has_more
    ? null
    : serverOverviewTimeline(page?.timeline, timelineMode.value, indexBySeq)
  if (serverTimeline) return serverTimeline
  const tiles = Array.isArray(page?.tiles) ? page.tiles : []
  if (tiles.length === 0) return null
  // A complete small page already has the real per-record timeline.  Tiles
  // are an overview fallback for a truncated page; using them for every
  // thread was the reason short sessions appeared as three solid bars.
  if (!truncated.value && !hasNewer.value) return null
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
  const cellsById = new Map(allRecords.value.map(record => [record.cell.recordId, record.cell]))
  const spanByIndex = new Map((timeline.value?.spans ?? []).map(span => [span.index, span]))
  const detailedBuckets = new Set<number>()
  const detailedRanges: Array<{ start: number; end: number }> = []
  const detailedSpans: OverviewSpan[] = []
  // Overlay the rows currently in the bounded ledger with their real
  // positions. The rest of the session remains tile based, so the first
  // screen has DSH-like event marks without unbounded DOM work.
  for (const row of trajectoryRows.value) {
    const cell = cellsById.get(String(row.item_id))
    const localSpan = cell ? spanByIndex.get(cell.index) : undefined
    if (!cell || !localSpan) continue
    const rawStart = timelineMode.value === 'sequence'
      ? Number(row.created_seq)
      : timestampMs(row.started_at)
    const completed = timelineMode.value === 'sequence'
      ? null
      : timestampMs(row.completed_at ?? row.started_at)
    const rawEnd = timelineMode.value === 'sequence'
      ? (Number.isFinite(rawStart) ? rawStart + 1 : NaN)
      : (rawStart !== null ? Math.max(rawStart, completed ?? rawStart) : NaN)
    if (rawStart === null || rawEnd === null
      || !Number.isFinite(rawStart) || !Number.isFinite(rawEnd)) continue
    const bucket = timelineMode.value === 'sequence'
      ? Math.floor(rawStart / bucketWidth)
      : null
    if (bucket !== null) {
      detailedBuckets.add(bucket)
    }
    detailedRanges.push({ start: rawStart, end: Math.max(rawEnd, rawStart + 0.001) })
    start = Math.min(start, rawStart)
    end = Math.max(end, rawEnd)
    detailedSpans.push({
      ...localSpan,
      start: rawStart,
      end: Math.max(rawEnd, rawStart + (timelineMode.value === 'sequence' ? 1 : 0.001)),
      count: 1,
    })
  }
  const perTileLimit = Math.max(1, Math.min(
    16,
    Math.floor(MAX_OVERVIEW_SPANS / Math.max(1, tiles.length)),
  ))
  const validTiles: Array<{ tile: any; bucket: number; total: number; start: number; end: number }> = []
  for (const tile of tiles) {
    const bucket = Number(tile?.bucket)
    const total = Math.max(0, Number(tile?.count) || 0)
    if (!Number.isFinite(bucket) || total <= 0) continue
    // Tile timestamps are stored as Unix seconds while the timed timeline
    // uses milliseconds. Compute the range before checking overlap; keeping
    // this above the guard also avoids a temporal-dead-zone failure in the
    // production bundle (the minifier used to call these values `Ne`).
    const rawStart = timelineMode.value === 'sequence'
      ? bucket * bucketWidth
      : timestampMs(tile?.start)
    const tileEnd = timelineMode.value === 'sequence'
      ? (bucket + 1) * bucketWidth
      : timestampMs(tile?.end)
    const rawEnd = timelineMode.value === 'sequence'
      ? tileEnd
      : (rawStart !== null ? Math.max(rawStart, tileEnd ?? rawStart) : null)
    if (rawStart === null || rawEnd === null
      || !Number.isFinite(rawStart) || !Number.isFinite(rawEnd)) continue
    validTiles.push({ tile, bucket, total, start: rawStart, end: rawEnd })
  }
  // Preserve the whole thread's domain even when every aggregate bucket is
  // covered by the currently loaded detailed rows. Without this pass, a long
  // thread can temporarily zoom into the tail window and make the timeline
  // appear to contain only a handful of records.
  for (const tile of validTiles) {
    start = Math.min(start, tile.start)
    end = Math.max(end, tile.end)
  }
  for (const entry of validTiles) {
    const { tile, bucket, total, start: rawStart, end: rawEnd } = entry
    // Never paint a full aggregate lane underneath real records from the
    // bounded ledger. The loaded records provide the useful detail; leaving
    // the remainder of that bucket empty is clearer than a misleading solid
    // background behind them.
    if (detailedBuckets.has(bucket)
      || (timelineMode.value !== 'sequence'
        && detailedRanges.some(range => range.start <= rawEnd && range.end >= rawStart))) continue
    const explicit = [Number(tile?.input_count) || 0, Number(tile?.model_count) || 0, Number(tile?.tool_count) || 0]
    const explicitTotal = explicit.reduce((sum, value) => sum + value, 0)
    const laneCounts = explicitTotal > 0
      ? explicit
      : [
          Math.round(total * ratios[0]),
          Math.round(total * ratios[1]),
          Math.max(0, total - Math.round(total * ratios[0]) - Math.round(total * ratios[1])),
        ]
    const spanEnd = Math.max(rawEnd, rawStart + (timelineMode.value === 'sequence' ? 1 : 0.001))
    const totalLaneCount = laneCounts.reduce((sum, value) => sum + Math.max(0, value), 0)
    const segmentCount = Math.max(1, Math.min(perTileLimit, totalLaneCount || total))
    const segmentWidth = (spanEnd - rawStart) / segmentCount
    for (let segment = 0; segment < segmentCount; segment += 1) {
      const eventAt = ((segment + 0.5) / segmentCount) * (totalLaneCount || total)
      let lane = 2
      let laneEnd = 0
      for (let candidate = 0; candidate < 3; candidate += 1) {
        laneEnd += Math.max(0, laneCounts[candidate] ?? 0)
        if (eventAt <= laneEnd) {
          lane = candidate
          break
        }
      }
      const segmentStart = rawStart + segment * segmentWidth
      const segmentEnd = segment === segmentCount - 1 ? spanEnd : segmentStart + segmentWidth
      spans.push({
        start: segmentStart,
        end: segmentEnd,
        index: -(spans.length + 1),
        isError: Number(tile?.failed) > 0,
        kind: lane === 0 ? 'context' : lane === 1 ? 'message' : 'tool',
        label: `${Math.max(1, Math.round(total / segmentCount))} events`,
        lane,
        ttftFraction: null,
        count: Math.max(1, Math.round(total / segmentCount)),
        aggregate: true,
        bucket,
      })
    }
  }
  const aggregateSpans = spans.splice(0)
  // Prioritize the loaded ledger window when the bounded cap is reached; it
  // is the only part users can inspect and click through immediately.
  spans.push(...detailedSpans, ...aggregateSpans.slice(
    0,
    Math.max(0, MAX_OVERVIEW_SPANS - detailedSpans.length),
  ))
  if (spans.length === 0 || !Number.isFinite(start) || !Number.isFinite(end)) return null
  return { start, end: Math.max(end, start + 1), spans, turnBoundaries: [] }
})

const domain = computed(() => {
  const model = overviewTimeline.value ?? timeline.value
  if (!model) return { start: 0, end: 1 }
  return { start: model.start, end: Math.max(model.end, model.start + 1) }
})

// Selection and click targets share the displayed overview coordinates.
// Recompressing only the loaded tail gives those same records another origin.
const interactionTimeline = computed(() => overviewTimeline.value
  ? { ...overviewTimeline.value, spans: overviewTimeline.value.spans.filter(span => span.index >= 0) }
  : timeline.value)

watch([timeline, overviewTimeline], ([detailModel, overviewModel]) => {
  const model = overviewModel ?? detailModel
  if (!model || !Number.isFinite(model.start) || !Number.isFinite(model.end)) {
    viewStart.value = 0
    viewSpan.value = 1
    return
  }
  viewStart.value = model.start
  viewSpan.value = Math.max(1, model.end - model.start)
}, { immediate: true })

const projectedOverviewSpans = computed(() => {
  const model = overviewTimeline.value
  if (!model) return []
  const width = trackWidth.value || 600
  const span = Math.max(1e-9, viewSpan.value)
  return model.spans.map((modelSpan, i) => {
    const left = ((modelSpan.start - viewStart.value) / span) * width
    const pw = ((modelSpan.end - modelSpan.start) / span) * width
    const gap = Math.min(pw * 0.15, 0.5)
    return {
      key: `overview-${modelSpan.index}-${i}`,
      model: modelSpan,
      left: left + gap,
      width: Math.max(1, pw - gap * 2),
      dimmed: false,
      outside: false,
    }
  }).filter(item => item.left + item.width > 0 && item.left < width)
})

/** 选中变化后，若对应 span 不在时间线视口内则自动平移视口（对齐参考实现）。 */
watch(selectedIndex, (index) => {
  if (index === null) return
  const model = interactionTimeline.value
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
  return new Set((interactionTimeline.value?.spans??[]).filter(s=>s.start<=end && s.end>=start).map(s=>s.index))
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
  if (overviewTimeline.value) return []
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
  if (!Number.isFinite(viewStart.value) || !Number.isFinite(span)) return []
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
    const params = {
      ...(cursor === null ? {} : { [direction === 'newer' ? 'after' : 'before']: cursor }),
      limit: TRAJECTORY_PAGE_ROWS }
    const response = await getThreadViewTrajectoryPage(sessionId, params, { signal: controller.signal })
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
    const window = mergeTrajectoryPage(trajectoryRows.value, page, direction, TRAJECTORY_WINDOW_ROWS,
      { before: beforeCursor.value, after: afterCursor.value, hasOlder: truncated.value, hasNewer: hasNewer.value })
    trajectoryRows.value = markRaw(window.rows)
    beforeCursor.value = window.before
    afterCursor.value = window.after
    truncated.value = window.hasOlder
    hasNewer.value = window.hasNewer
    displayPage.value = markRaw(page)
    detailRefById.clear()
    for (const row of trajectoryRows.value) detailRefById.set(row.item_id, row.detail)
    rawTurns.value = markRaw(trajectoryPageTurns(trajectoryRows.value))
    // Auxiliary state must have the same lifetime as the bounded ledger.
    const rounds = new Set(trajectoryRows.value.map(row => row.user_round))
    collapsedTurns.value = new Set([...collapsedTurns.value].filter(round => rounds.has(round)))
    const groups = collapsibleAssistantKeys.value
    collapsedAssistants.value = new Set([...collapsedAssistants.value].filter(key => groups.has(key)))
    selectedIndex.value = direction && selectedId
      ? allRecords.value.find(record => record.cell.recordId === selectedId)?.cell.index ?? null : null
    if (selectedIndex.value === null) details.value = {}
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
  const cursor = direction === 'older' ? beforeCursor.value : afterCursor.value
  if (cursor == null) return
  await load(cursor, direction)
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

watch(() => route.query.session, () => {
  detailAbort?.abort()
  detailAbort = null
  detailLoading.value = false
  detailError.value = false
  detailCache.clear()
  detailRefById.clear()
  detailRequestSerial++
  trajectoryRows.value = []
  beforeCursor.value = null
  afterCursor.value = null
  truncated.value = false
  hasNewer.value = false
  rawTurns.value = []
  displayPage.value = null
  details.value = {}
  searchQuery.value = ''
  searchAbort?.abort()
  collapsedTurns.value = new Set()
  collapsedAssistants.value = new Set()
  void load()
})

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
  detailAbort = null
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
    'tt-row-selected': !record.collapsedSummary && selectedCell.value?.index === record.cell.index,
    'tt-row-turn-start': record.turnStart,
    'tt-row-turn-end': record.turnEnd,
    'tt-row-summary': Boolean(record.collapsedSummary),
    [`tt-row-${record.cell.kind}`]: true,
    'tt-row-error': !record.collapsedSummary && record.cell.isError === true,
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
  return record.collapsedSummary ? null : dotsByAssistantIndex.value.get(record.cell.index) ?? null
}

function onRowDoubleClick(record: TableRecord): void {
  if (record.collapsedSummary) return
  if (collapsedTurns.value.has(record.turn)) { toggleTurn(record.turn); return }
  const key = trajectoryRecordId(record.cell)
  if (collapsibleAssistantKeys.value.has(key)) {
    const next = new Set(collapsedAssistants.value)
    if (next.has(key)) next.delete(key)
    else next.add(key)
    collapsedAssistants.value = next
  } else if (record.turnStart) toggleTurn(record.turn)
}

watch(allRecords, () => {
  collapsedTurns.value = new Set([...collapsedTurns.value].filter(turn => turn !== null && collapsibleTurnKeys.value.has(turn)))
  collapsedAssistants.value = new Set([...collapsedAssistants.value].filter(key => collapsibleAssistantKeys.value.has(key)))
})

/* ---------------------------------------------------------------- */
/* 选中与详情                                                        */
/* ---------------------------------------------------------------- */

function selectIndex(index: number, options: { fromLedger?: boolean } = {}): void {
  selectedIndex.value = index
  const cell = findCell(index)
  activeTab.value = defaultTabFor(cell)
  openSections.input = true
  openSections.output = true
  void hydrateDetail(cell)
  // 台账内点击不需要滚动；时间线与跳转链接的选中要把目标行带到可视区并高亮。
  if (!options.fromLedger) revealRecord(index)
}

function detailBodyRef(cell: TrajCell | null): { body_id: string; revision: number } | null {
  if (!cell) return null
  const value = detailRefById.get(cell.recordId)
  if (!value || typeof value !== 'object') return null
  const bodyId = typeof value.body_id === 'string' ? value.body_id : ''
  const revision = Number(value.revision)
  if (!bodyId || !Number.isSafeInteger(revision) || revision < 0) return null
  return { body_id: bodyId, revision }
}

function boundedDetailCacheSet(key: string, revision: number, value: unknown): void {
  detailCache.delete(key)
  detailCache.set(key, { revision, value })
  // A user can click through thousands of rows in a long trajectory. Keep a
  // small LRU so detail hydration never becomes another unbounded history
  // cache.
  while (detailCache.size > 128) detailCache.delete(detailCache.keys().next().value as string)
}

async function hydrateDetail(cell: TrajCell | null): Promise<void> {
  detailAbort?.abort()
  detailAbort = null
  detailLoading.value = false
  detailError.value = false
  details.value = {}
  const serial = ++detailRequestSerial
  if (!cell) return
  const direct = detailRefById.get(cell.recordId)
  if (direct && typeof direct === 'object' && !('body_id' in direct) && !('bodyId' in direct)) {
    details.value = { ...details.value, [cell.recordId]: direct }
    detailError.value = false
    return
  }
  const body = detailBodyRef(cell)
  if (!body) return
  const sessionId = String(route.query.session ?? '').trim()
  const key = `${sessionId}:${cell.recordId}:${body.revision}`
  const cached = detailCache.get(key)
  if (cached && cached.revision === body.revision) {
    details.value = { ...details.value, [cell.recordId]: cached.value }
    detailError.value = false
    return
  }

  const controller = new AbortController()
  detailAbort = controller
  detailLoading.value = true
  try {
    const chunks: string[] = []
    let cursor = 0
    let totalBytes = 0
    for (;;) {
      const response = await getThreadViewBody(
        sessionId,
        body.body_id,
        { revision: body.revision, cursor },
        { signal: controller.signal, timeout: DETAIL_REQUEST_TIMEOUT_MS },
      )
      if (controller.signal.aborted || detailAbort !== controller || serial !== detailRequestSerial) return
      const page = response.data.data
      const pageChunks = Array.isArray(page?.chunks) ? page.chunks : []
      for (const chunk of pageChunks) {
        if (typeof chunk !== 'string') continue
        totalBytes += chunk.length
        if (totalBytes > DETAIL_MAX_BYTES) throw new Error('detail body too large')
        chunks.push(chunk)
      }
      if (!page?.has_more) break
      const next = Number(page?.next_cursor)
      if (!Number.isSafeInteger(next) || next <= cursor) throw new Error('invalid detail cursor')
      cursor = next
    }
    const text = chunks.join('')
    let value: unknown
    try { value = JSON.parse(text) } catch { throw new Error('invalid detail payload') }
    if (controller.signal.aborted || detailAbort !== controller || serial !== detailRequestSerial) return
    boundedDetailCacheSet(key, body.revision, value)
    details.value = { ...details.value, [cell.recordId]: value }
  } catch {
    if (!controller.signal.aborted && detailAbort === controller && serial === detailRequestSerial) {
      detailError.value = true
    }
  } finally {
    if (detailAbort === controller) {
      detailAbort = null
      detailLoading.value = false
    }
  }
}

function retrySelectedDetail(): void {
  void hydrateDetail(selectedIndex.value === null ? null : findCell(selectedIndex.value))
}

/** 把选中的记录滚动到台账可视区中部（对齐参考实现的滚动定位）。 */
function revealRecord(index: number): void {
  const record = allRecords.value.find(entry => entry.cell.index === index)
  if (!record) return
  const assistantKey = foldGroups.value.toolOwners.get(index)
  let changed = false
  if (collapsedTurns.value.has(record.turn)) {
    const next = new Set(collapsedTurns.value)
    next.delete(record.turn)
    collapsedTurns.value = next
    changed = true
  }
  if (assistantKey && collapsedAssistants.value.has(assistantKey)) {
    const next = new Set(collapsedAssistants.value)
    next.delete(assistantKey)
    collapsedAssistants.value = next
    changed = true
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
  const payload = detail && typeof detail === 'object' && detail.payload && typeof detail.payload === 'object'
    ? { ...(detail as Record<string, unknown>), ...(detail.payload as Record<string, unknown>) }
    : detail
  const source = payload as any
  const detailedFailure = trajectoryFailure(source, base.status)
  // Legacy terminal failures are folded into the assistant summary. Its own
  // old body may contain no diagnostics; hydration must not erase that repair.
  const failure = base.failure || detailedFailure
    ? { ...base.failure, ...Object.fromEntries(Object.entries(detailedFailure ?? {}).filter(([, value]) => value !== undefined)) }
    : undefined
  const runtime = source?.runtime && typeof source.runtime === 'object' ? source.runtime : null
  const runtimeState = runtime?.agent_state && typeof runtime.agent_state === 'object'
    ? runtime.agent_state : null
  const usage = extractUsage(source) ?? base.usage
  const metrics = usage || base.metrics
    ? {
        ...(base.metrics ?? {
          timingRecorded: false,
          stepStartTime: base.startedAt,
          firstTokenTime: null,
          completedTime: null,
          usageProvided: false,
          outputTokens: null,
        }),
        usageProvided: usage !== undefined,
        outputTokens: usage?.output ?? base.metrics?.outputTokens ?? null,
      }
    : base.metrics
  const args = source?.args ?? source?.arguments
  const content = source?.content ?? source?.message
  const compactionSummary = [source?.summary_text, source?.summaryText, source?.summary_model_output,
    source?.summaryModelOutput, source?.summary, content, base.outputDetail]
    .find(value => typeof value === 'string' && value.trim() && value.trim() !== 'history')
  const resultValue = source?.result ?? source?.data ?? source?.output ?? source?.raw
  const schema = source?.schema ?? source?.tool_schema ?? source?.input_schema ?? source?.inputSchema
  const stringify=(v:unknown)=>typeof v==='string'?v:v===undefined||v===null?'':JSON.stringify(v,null,2)
  return {
    ...base,
    failure,
    retry: trajectoryRetry(source) ?? base.retry,
    // Assistant rows only have their response in the durable detail payload.
    // Do not use `content` as a synthetic input: that produced identical
    // Input/Output sections in the overview. Tool rows still use their args.
    inputDetail: base.kind === 'subtool'
      ? stringify(source?.task ?? source?.user_message ?? runtime?.user_message
        ?? runtime?.task ?? runtimeState?.user_message ?? base.inputDetail)
      : base.kind === 'tool'
        ? stringify(args ?? base.inputDetail)
      : undefined,
    outputDetail: stringify(base.kind === 'subtool'
      ? (source?.summary ?? source?.latest_message ?? runtime?.summary
        ?? runtime?.latest_message ?? runtimeState?.message
        ?? runtimeState?.assistant_message ?? base.outputDetail)
      : base.kind === 'compacted'
        ? (compactionSummary ?? base.outputDetail)
        : (content ?? base.outputDetail)),
    thinkingDetail:stringify(
      source?.reasoning || source?.reasoning_content || source?.thinking || source?.thought
        || source?.reasoningText || source?.thinking_content || base.thinkingDetail
    ),
    result:stringify(resultValue ?? base.result),
    schemaDetail:stringify(schema ?? base.schemaDetail),
    toolName: base.kind === 'subtool'
      ? (typeof source?.label === 'string' ? source.label
        : typeof runtime?.label === 'string' ? runtime.label
          : typeof runtime?.spawn_label === 'string' ? runtime.spawn_label : base.toolName)
      : (typeof source?.tool === 'string' ? source.tool : (typeof source?.name === 'string' ? source.name : base.toolName)),
    usage,
    metrics,
  }
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
  if (cell.status === 'cancelled' || cell.status === 'interrupted') return t('messenger.trajectory.stateInterrupted')
  if (cell.status === 'rejected') return t('messenger.trajectory.stateRejected')
  if (cell.isError) return t('messenger.trajectory.stateFailed')
  if (cell.retry?.willRetry && cell.status === 'running') return t('messenger.trajectory.stateRetrying')
  if (cell.status === 'running' || cell.status === 'queued') return t('messenger.trajectory.stateWaiting')
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
  if (cell.kind === 'compacted') {
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
    // Timing already rides the overview (`总时长`), and the other two surfaces
    // never offered a second copy of it on a tool entry.
    return [
      overviewTab.value,
      paramsTab.value,
      resultTab.value,
      schemaTab.value,
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
    const model = interactionTimeline.value
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
  void hydrateDetail(null)
}

function spanClasses(span: { model: TimelineSpan; dimmed: boolean; outside: boolean }): Record<string, boolean> {
  return {
    [`tt-span-${span.model.kind}`]: true,
    'tt-span-error': span.model.isError,
    'tt-span-dimmed': span.dimmed,
    'tt-span-outside': span.outside,
    'tt-span-selected': selectedIndex.value !== null && selectedIndex.value === span.model.index,
  }
}

function spanStyle(span: { model: TimelineSpan; left: number; width: number }): Record<string, string> {
  const style: Record<string, string> = {
    left: `${span.left}px`,
    width: `${span.width}px`,
    top: `${7 + span.model.lane * 14}px`,
  }
  if ((span.model as OverviewSpan).aggregate) {
    // Keep aggregate overview bars at the same strength as the
    // `.tt-span-overview` rule and the native Slint trajectory.
    style.opacity = '0.94'
    style.pointerEvents = 'auto'
  }
  return style
}

function onRowClick(record: TableRecord): void {
  if (record.collapsedSummary) {
    if (record.collapsedSummaryKind === 'turn' && record.turn !== null) {
      const next = new Set(collapsedTurns.value)
      next.delete(record.turn)
      collapsedTurns.value = next
    } else {
      const next = new Set(collapsedAssistants.value)
      next.delete(trajectoryRecordId(record.cell))
      collapsedAssistants.value = next
    }
    return
  }
  selectIndex(record.cell.index, { fromLedger: true })
}

function overviewSpanTitle(span: { model: TimelineSpan }): string {
  if (!(span.model as OverviewSpan).aggregate) return span.model.label || '记录'
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

/** 直方图与汇总的紧凑 token 数：12,345 → 12.3k。 */
function formatCompactTokens(value: number): string {
  if (!Number.isFinite(value)) return '0'
  if (value >= 1e6) return `${(value / 1e6).toFixed(1)}M`
  if (value >= 1e3) return `${(value / 1e3).toFixed(1)}k`
  return String(Math.round(value))
}

function tokenBarHeight(bar: StepUsageBar): string {
  if (!bar.reported) return '2px'
  const ratio = stepUsage.value.max > 0 ? (bar.delta ?? 0) / stepUsage.value.max : 0
  return `${Math.max(4, Math.round(ratio * 100))}%`
}

function tokenBarClasses(bar: StepUsageBar): Record<string, boolean> {
  return {
    'tt-tokenbar-empty': !bar.reported,
    'tt-tokenbar-max': bar.reported && bar.delta !== null && bar.delta === stepUsage.value.max && bar.delta > 0,
    'tt-tokenbar-selected': selectedIndex.value !== null && selectedIndex.value === bar.assistantIndex,
  }
}

function onTokenBarEnter(bar: StepUsageBar, event: PointerEvent): void {
  if (tooltipTimer !== null) window.clearTimeout(tooltipTimer)
  const clientX = event.clientX
  const clientY = event.clientY
  tooltipTimer = window.setTimeout(() => {
    const stepText = modelT('trajectory.stepLabel', { step: bar.step })
    const lines: string[] = [
      bar.turn !== null ? `${modelT('trajectory.turnTitle', { turn: bar.turn })} · ${stepText}` : stepText,
    ]
    if (bar.reported) {
      lines.push(modelT('trajectory.usageDelta', {
        value: modelT('trajectory.tokens', { value: formatCompactTokens(bar.delta ?? 0) }),
      }))
      lines.push(modelT('trajectory.usageStepTotal', {
        value: modelT('trajectory.tokens', { value: formatCompactTokens(bar.total) }),
      }))
      const parts = [
        modelT('trajectory.usageInput', { value: formatCompactTokens(bar.input) }),
        modelT('trajectory.usageOutput', { value: formatCompactTokens(bar.output) }),
      ]
      if (bar.cacheRead > 0) {
        parts.push(modelT('trajectory.usageCacheRead', { value: formatCompactTokens(bar.cacheRead) }))
      }
      if (bar.cacheWrite > 0) {
        parts.push(modelT('trajectory.usageCacheWrite', { value: formatCompactTokens(bar.cacheWrite) }))
      }
      lines.push(parts.join(' · '))
    } else {
      lines.push(modelT('trajectory.usageNotReported'))
    }
    if (bar.startedAt !== null && bar.completedAt !== null && bar.completedAt >= bar.startedAt) {
      lines.push(modelT('trajectory.totalDurationValue', {
        duration: formatDurationMillis(bar.completedAt - bar.startedAt, modelT),
      }))
    }
    tooltip.lines = lines
    tooltip.x = clientX
    tooltip.y = clientY
    tooltip.visible = true
  }, TOOLTIP_DELAY_MS)
}

function onTokenBarClick(bar: StepUsageBar): void {
  selectIndex(bar.assistantIndex)
}
</script>

<style scoped>
.tt-diagnostic-row dd {
  white-space: pre-wrap;
  max-height: 180px;
  overflow: auto;
}
</style>
