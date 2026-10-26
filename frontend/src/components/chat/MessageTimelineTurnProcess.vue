<template>
  <!--
    轻量展示窗（线程视图）的工作过程折叠条：与完整投影的 MessageTimelineBlocks
    过程条同一形态（箭头沟槽 + 标题 + 步数），默认收起；首次展开时按需拉取该
    用户轮次的持久条目（getThreadLogTurn），就地渲染思考 / 工具 / 中间打印，
    收起即卸载 DOM。不再跳转轨迹视图。
  -->
  <div class="timeline-turn-process" :class="{ 'is-open': open }">
    <button
      class="timeline-process-head"
      data-turn-slot="process"
      :class="{ 'is-open': open }"
      type="button"
      :aria-expanded="open"
      @click="toggle"
    >
      <span class="timeline-process-gutter" aria-hidden="true">
        <i :class="['fa-solid', 'fa-caret-right', 'timeline-process-arrow', { 'is-open': open }]"></i>
      </span>
      <span class="timeline-process-title">{{ t('chat.timeline.workProcess') }}</span>
      <span class="timeline-process-count">{{ t('chat.timeline.workProcessSteps', { count }) }}</span>
      <span class="timeline-process-latest">{{ currentActivity }}</span>
    </button>

    <div v-if="open" ref="processBodyRef" class="timeline-process-body" @scroll.passive="handleProcessScroll" @wheel.passive="handleProcessWheel" @pointerdown="pageAnchor = null" @keydown="pageAnchor = null">
      <div v-if="loading && !loaded" class="timeline-process-note">{{ t('chat.timeline.processLoading') }}</div>
      <template v-else-if="error">
        <div class="timeline-process-note">{{ t('chat.timeline.processFailed') }}</div>
        <button class="timeline-process-retry" type="button" @click="load">{{ t('chat.timeline.processRetry') }}</button>
      </template>
      <template v-else>
        <div v-if="hasOlderItems" class="timeline-process-page-state timeline-process-page-state-top" role="status">
          {{ pageError ? t('chat.timeline.processPageFailed') : loadingMore ? t('chat.timeline.processLoading') : t('chat.timeline.processScrollEarlier') }}
        </div>
        <div class="timeline-group">
        <div v-for="row in rows" :key="row.key" :data-process-key="row.key">
          <MessageTimelineThinkingEntry
            v-if="row.kind === 'reasoning'"
            :entry="row.entry"
            :open="openKeys.has(row.key)"
            @toggle="toggleKey(row.key)"
          />
          <MessageTimelineToolEntry
            v-else-if="row.kind === 'tool'"
            :entry="row.entry"
            :open="openKeys.has(row.key)"
            :patch-view="patchViewFor(row.entry)"
            :structured-view="structuredViewFor(row.entry)"
            @toggle="toggleKey(row.key)"
          />
          <div v-else class="timeline-body-block" data-turn-slot="body">
            <MessageMarkdownBody
              :cache-key="`${cacheKeyPrefix}${row.key}`"
              :content="row.text"
              :explicit-content="true"
              :assistant-display="true"
              @rendered="restorePageAnchor"
            />
          </div>
        </div>
        </div>
        <!-- Live reasoning belongs after the durable process rows until its own
             record arrives.  Rendering it before the list made each new model
             round appear to overwrite the first completed reasoning entry. -->
        <MessageTimelineThinkingEntry
          v-if="reasoningText && !hasCurrentReasoningRow && !hasNewerItems"
          :entry="reasoningEntry"
          :open="openKeys.has(currentReasoningRowKey)"
          @toggle="toggleKey(currentReasoningRowKey)"
        />
        <div v-if="hasNewerItems" class="timeline-process-page-state" role="status">
          {{ pageError ? t('chat.timeline.processPageFailed') : loadingMore ? t('chat.timeline.processLoading') : t('chat.timeline.processScrollLater') }}
        </div>
        <!-- 有工具轮次的流式尾部：正文区在流式期间让位，活动文本在这里直播，
             轮次完成后由正文区接管（buildRows 会排除最后一条助手正文，不重复）。 -->
        <div v-if="liveTail && !hasNewerItems" class="timeline-body-block timeline-live-print">{{ liveTail }}</div>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, triggerRef, watch } from 'vue';

import MessageMarkdownBody from './MessageMarkdownBody.vue';
import MessageTimelineThinkingEntry from './MessageTimelineThinkingEntry.vue';
import MessageTimelineToolEntry from './MessageTimelineToolEntry.vue';
import { t } from '@/i18n';
import { getThreadLogTurn } from '@/api/chat';
import {
  buildTimelineToolEntry,
  type TimelineReasoningEntry,
  type TimelineToolEntry
} from './toolTimelineModel';
import { buildWorkflowToolRuns, type WorkflowItem } from './toolWorkflowRunModel';
import { buildTimelinePatchView } from './toolTimelinePatch';
import { buildTimelineStructuredView } from './toolTimelineStructured';
import type { ToolWorkflowPatchView, ToolWorkflowStructuredView } from './toolWorkflowTypes';
import { entryActivity, processActivityLabel, reasoningPreview, type ProcessActivity } from './timelineActivity';
import { completeProcessRefresh, requestProcessRefresh, type ProcessRefreshGateState } from './processRefreshGate';

type ProcessRow =
  | { kind: 'reasoning'; key: string; entry: TimelineReasoningEntry; modelRound?: number }
  | { kind: 'tool'; key: string; entry: TimelineToolEntry }
  | { kind: 'print'; key: string; text: string };

const props = defineProps<{
  sessionId: string;
  turnId: string;
  count: number;
  activity?: ProcessActivity | null;
  status?: string;
  /** 流式期间的活动文本尾部：正文区让位时由过程区实时行承接展示。 */
  liveTail?: string;
  reasoning?: string;
  reasoningItemId?: string;
  /** The item-scoped reasoning stream may still be active after the process log loads. */
  reasoningStreaming?: boolean;
  /** Stable terminal assistant item from the chat view row. The lazy log can
   * contain several assistant prints; only this item belongs in the answer
   * block, all earlier prints stay in the work process. */
  finalItemId?: string;
}>();

// 展开状态上报：父组件据此决定流式尾部是否外显（过程区收起时正文已让位，
// tail 若再不外显，流式期间用户将看不到任何输出）。
const emit = defineEmits<{ 'open-change': [open: boolean] }>();

const open = ref(false);
watch(open, (value) => emit('open-change', value));
const loading = ref(false);
const error = ref(false);
const loaded = ref(false);
const rows = shallowRef<ProcessRow[]>([]);
/** A work process can be much larger than a message body. Keep its retained
 * data and DOM bounded; both edges are independently keyset-paged. */
const PROCESS_ITEM_PAGE_SIZE = 100;
const PROCESS_ITEM_WINDOW = 400;
const hasOlderItems = ref(false);
const hasNewerItems = ref(false);
const loadingMore = ref(false);
const pageError = ref(false);
const firstItemIndex = ref<number | null>(null);
const lastItemIndex = ref<number | null>(null);
const processItems = shallowRef<Array<Record<string, unknown>>>([]);
const openKeys = ref(new Set<string>());
const processBodyRef = ref<HTMLElement | null>(null);
const patchViews = new Map<string, ToolWorkflowPatchView | null>();
const structuredViews = new Map<string, ToolWorkflowStructuredView | null>();
let loadAbort: AbortController | null = null;
let reloadTimer: number | null = null;
const refreshGate: ProcessRefreshGateState = { pending: false };

const cacheKeyPrefix = computed(() => `turn-process:${props.sessionId}:${props.turnId}:`);
const reasoningText = computed(() => String(props.reasoning || '').trim());
const currentActivity = computed(() => {
  let activity = props.activity;
  if (!activity) {
    if (props.reasoningStreaming) activity = { kind: 'thinking' };
    else if (!hasNewerItems.value) {
      for (let index = rows.value.length - 1; index >= 0; index--) {
        const row = rows.value[index];
        if (row.kind === 'print') continue;
        activity = entryActivity(row.entry);
        if (activity) break;
      }
    }
  }
  return processActivityLabel(activity, props.status || (props.reasoningStreaming ? 'running' : 'completed'), t);
});
/** The server gives the current reasoning item's durable identity.  Keep a
 * local fallback only for an older source that does not expose it yet. */
const currentReasoningItemId = computed(() => String(props.reasoningItemId || '').trim());
const fallbackReasoningKey = ref('projected:0');
const currentReasoningRowKey = computed(() => `reasoning:${currentReasoningItemId.value || fallbackReasoningKey.value}`);
/**
 * The compact chat row and the lazy process endpoint are published by different
 * projections.  During a running request the compact row can briefly have a
 * newer item id than the already persisted process item, even though both hold
 * the same thought.  Bind that durable row once by its text so the live preview
 * updates it in place instead of adding a second, identical thinking entry.
 *
 * This is deliberately a binding, rather than a general content de-duplication:
 * two completed model rounds are allowed to contain the same words and must
 * remain two separate process records.
 */
const currentReasoningMatchKey = ref('');
const currentReasoningMatchIdentity = ref('');
/**
 * A live preview may be addressed by the execution item id while the lazy
 * process endpoint returns a compatibility/random item id.  Both records
 * still carry the same model round.  The component is already scoped to one
 * user turn, so the round number is the stable identity we can safely use for
 * this hand-off; text is only a last-resort guard for older rows.
 */
const modelRoundFromIdentity = (value: string): number | null => {
  const match = value.match(/(?:^|:)(?:text|reasoning-text|reasoning)[-_](\d+)$/i);
  if (!match) return null;
  const round = Number(match[1]);
  return Number.isSafeInteger(round) && round >= 0 ? round : null;
};
const modelRoundFromReasoningRow = (row: ProcessRow): number | null => {
  if (row.kind !== 'reasoning') return null;
  if (row.modelRound !== undefined && Number.isSafeInteger(row.modelRound) && row.modelRound >= 0) {
    return row.modelRound;
  }
  return modelRoundFromIdentity(row.key.slice('reasoning:'.length));
};
const sameCurrentReasoningIdentity = (row: ProcessRow, liveIdentity: string): boolean => {
  if (row.kind !== 'reasoning') return false;
  const liveRound = modelRoundFromIdentity(liveIdentity);
  const persistedRound = modelRoundFromReasoningRow(row);
  return liveRound !== null && persistedRound !== null && liveRound === persistedRound;
};
const sameCurrentReasoningRound = (row: ProcessRow, liveRound: number | null): boolean =>
  liveRound !== null && modelRoundFromReasoningRow(row) === liveRound;
const sameCurrentReasoningText = (persisted: string, live: string): boolean => {
  if (persisted === live) return true;
  // While a tail is active, either projection may be one durable flush behind.
  // Accept only a prefix relation here; once bound, later deltas do not rescan
  // all rows and therefore cannot accidentally merge an older completed round.
  return Boolean(props.reasoningStreaming) && (persisted.startsWith(live) || live.startsWith(persisted));
};
const bindCurrentReasoningRow = (): void => {
  const live = reasoningText.value;
  const identity = currentReasoningRowKey.value;
  const liveRound = Number.isSafeInteger(Number(props.reasoningItemId)) ? Number(props.reasoningItemId) : null;
  if (!live) {
    currentReasoningMatchKey.value = '';
    currentReasoningMatchIdentity.value = '';
    return;
  }
  if (currentReasoningMatchIdentity.value !== identity) {
    currentReasoningMatchIdentity.value = identity;
    currentReasoningMatchKey.value = '';
  }
  if (rows.value.some((row) => row.kind === 'reasoning' && row.key === identity)) {
    currentReasoningMatchKey.value = identity;
    return;
  }
  if (currentReasoningMatchKey.value && rows.value.some((row) =>
    row.kind === 'reasoning' && row.key === currentReasoningMatchKey.value
  )) return;
  // Process items are chronological.  On an identity hand-off the most recent
  // equal thought is the only viable durable counterpart of the live preview.
  for (let index = rows.value.length - 1; index >= 0; index -= 1) {
    const row = rows.value[index];
    if (!sameCurrentReasoningIdentity(row, currentReasoningItemId.value) &&
        !sameCurrentReasoningRound(row, liveRound) &&
        !(row.kind === 'reasoning' && sameCurrentReasoningText(row.entry.text, live))) continue;
    currentReasoningMatchKey.value = row.key;
    return;
  }
  currentReasoningMatchKey.value = '';
};
const hasCurrentReasoningRow = computed(() => Boolean(currentReasoningMatchKey.value) &&
  rows.value.some((row) => row.kind === 'reasoning' && row.key === currentReasoningMatchKey.value)
);
const reasoningEntry = computed<TimelineReasoningEntry>(() => ({
  kind: 'reasoning', key: currentReasoningRowKey.value, streaming: Boolean(props.reasoningStreaming),
  summary: reasoningPreview(reasoningText.value), text: reasoningText.value
}));

const clearReloadTimer = (): void => {
  if (reloadTimer !== null) {
    window.clearTimeout(reloadTimer);
    reloadTimer = null;
  }
};

const toggle = (): void => {
  open.value = !open.value;
  if (open.value && !loaded.value && !loading.value) {
    clearReloadTimer();
    void load();
  } else if (open.value && loaded.value && refreshGate.pending) {
    clearReloadTimer();
    if (requestProcessRefresh(refreshGate, !loadAbort && !hasNewerItems.value)) void loadPage('refresh');
  }
};

const toggleKey = (key: string): void => {
  const next = new Set(openKeys.value);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  openKeys.value = next;
};

const patchViewFor = (entry: TimelineToolEntry): ToolWorkflowPatchView | null => {
  if (!patchViews.has(entry.key)) patchViews.set(entry.key, buildTimelinePatchView(entry.run, t));
  return patchViews.get(entry.key) ?? null;
};

const structuredViewFor = (entry: TimelineToolEntry): ToolWorkflowStructuredView | null => {
  if (!structuredViews.has(entry.key)) structuredViews.set(entry.key, buildTimelineStructuredView(entry.run, t));
  return structuredViews.get(entry.key) ?? null;
};

const itemIndex = (item: Record<string, unknown> | undefined): number | null => {
  const value = item?.item_index;
  return typeof value === 'number' && Number.isSafeInteger(value) ? value : null;
};

const rebuildRows = (): void => {
  rows.value = buildRows(processItems.value);
  bindCurrentReasoningRow();
  firstItemIndex.value = itemIndex(processItems.value[0]);
  lastItemIndex.value = itemIndex(processItems.value.at(-1));
  const keys = new Set(rows.value.map(row => row.key));
  openKeys.value = new Set([...openKeys.value].filter(key => keys.has(key) || key === currentReasoningRowKey.value));
  // Derived tool views are tied to a row revision. Drop them when a new page
  // arrives so neither an evicted row nor an outdated result stays cached.
  patchViews.clear();
  structuredViews.clear();
};

type ProcessPageDirection = 'initial' | 'older' | 'newer' | 'refresh';
let pageRetryAfter = 0;
let fillFrame: number | null = null;
let fillBudget = 4;
let pageAnchor: { key?: string; offset: number; follow: boolean } | null = null;
let anchorFrame: number | null = null;
let restoredScrollTop = 0;
let followLatest = true;
let liveFrame: number | null = null;
const followLiveTail = (): void => {
  if (!open.value || hasNewerItems.value || !followLatest || liveFrame !== null) return;
  liveFrame = window.requestAnimationFrame(() => {
    liveFrame = null;
    const body = processBodyRef.value;
    if (!body || !followLatest) return;
    body.scrollTop = body.scrollHeight;
    restoredScrollTop = body.scrollTop;
  });
};
watch(() => [reasoningText.value, props.reasoningStreaming, props.liveTail], followLiveTail, { flush: 'post' });
// Markdown renders asynchronously. Keep the same anchor through those height
// changes too, until the reader scrolls again, instead of anchoring empty blocks.
const restorePageAnchor = (): void => {
  if (anchorFrame !== null || !pageAnchor) return;
  anchorFrame = window.requestAnimationFrame(() => {
    anchorFrame = null;
    const body = processBodyRef.value;
    if (!body || !pageAnchor) return;
    if (pageAnchor.follow) body.scrollTop = body.scrollHeight;
    else {
      const row = [...body.querySelectorAll<HTMLElement>('[data-process-key]')]
        .find(node => node.dataset.processKey === pageAnchor?.key);
      if (row) body.scrollTop += row.getBoundingClientRect().top - body.getBoundingClientRect().top - pageAnchor.offset;
    }
    restoredScrollTop = body.scrollTop;
  });
};
const fillShortPage = (): void => {
  if (fillFrame !== null || !open.value || fillBudget <= 0) return;
  fillFrame = window.requestAnimationFrame(() => {
    fillFrame = null;
    const body = processBodyRef.value;
    if (!body || body.scrollHeight > body.clientHeight + 2 || loadAbort) return;
    const direction = hasOlderItems.value ? 'older' : hasNewerItems.value ? 'newer' : null;
    if (direction) { fillBudget--; void loadPage(direction); }
  });
};

const loadPage = async (direction: ProcessPageDirection): Promise<void> => {
  const sessionId = props.sessionId.trim();
  const turnId = props.turnId.trim();
  if (!sessionId || !turnId || loadAbort || !open.value) return;
  const initial = direction === 'initial';
  const older = direction === 'older';
  const refresh = direction === 'refresh';
  if (older && (!hasOlderItems.value || firstItemIndex.value === null)) return;
  if (direction === 'newer' && (!hasNewerItems.value || lastItemIndex.value === null)) return;
  if (refresh && hasNewerItems.value) return; // Never move a reader away from history.
  const controller = new AbortController();
  loadAbort = controller;
  loading.value = initial;
  loadingMore.value = !initial;
  error.value = false;
  pageError.value = false;
  try {
    // A high exclusive bound requests the latest page with an indexed DESC
    // query. Subsequent pages use the actual first/last durable item index.
    const params = initial ? { item_before: Number.MAX_SAFE_INTEGER, limit: PROCESS_ITEM_PAGE_SIZE }
      : older ? { item_before: firstItemIndex.value, limit: PROCESS_ITEM_PAGE_SIZE }
      : { item_after: refresh ? (itemIndex(processItems.value.at(-20)) ?? firstItemIndex.value ?? 0) - 1 : lastItemIndex.value,
          limit: PROCESS_ITEM_PAGE_SIZE };
    const response = await getThreadLogTurn(sessionId, turnId, params, { signal: controller.signal });
    if (controller.signal.aborted) return;
    const turn = response.data?.data?.turn;
    if (!Array.isArray(turn?.items)) throw new Error('Invalid process page');
    const batch = turn.items as Array<Record<string, unknown>>;
    // Capture geometry immediately before applying the page, after any user
    // scrolling during the request. Total-height deltas cannot anchor a window
    // that adds rows at one edge and evicts rows at the other.
    const body = processBodyRef.value;
    const top = body?.getBoundingClientRect().top ?? 0;
    const anchor = body && [...body.querySelectorAll<HTMLElement>('[data-process-key]')]
      .find(node => node.getBoundingClientRect().bottom > top);
    const anchorKey = anchor?.dataset.processKey;
    const anchorOffset = anchor ? anchor.getBoundingClientRect().top - top : 0;
    const follow = initial || (refresh && body && body.scrollHeight - body.clientHeight - body.scrollTop <= 72);
    pageAnchor = { key: anchorKey, offset: anchorOffset, follow: Boolean(follow) };
    followLatest = Boolean(follow);
    if (initial) {
      processItems.value = batch;
      hasOlderItems.value = Boolean(turn.has_older ?? turn.has_more);
      hasNewerItems.value = Boolean(turn.has_newer);
    } else if (batch.length) {
      const byId = new Map(processItems.value.map(item => [String(item.item_id), item]));
      for (const item of batch) byId.set(String(item.item_id), item);
      const merged = [...byId.values()].sort((a, b) => (itemIndex(a) ?? 0) - (itemIndex(b) ?? 0));
      const evicted = merged.length > PROCESS_ITEM_WINDOW;
      processItems.value = older ? merged.slice(0, PROCESS_ITEM_WINDOW) : merged.slice(-PROCESS_ITEM_WINDOW);
      if (older) {
        hasOlderItems.value = Boolean(turn.has_older ?? turn.has_more);
        hasNewerItems.value ||= evicted;
      } else {
        hasNewerItems.value = Boolean(turn.has_newer ?? turn.has_more);
        hasOlderItems.value ||= evicted;
      }
    } else if (older) hasOlderItems.value = false;
    else hasNewerItems.value = false;
    rebuildRows();
    loaded.value = true;
    await nextTick();
    if (controller.signal.aborted || !body) return;
    if (follow) body.scrollTop = body.scrollHeight;
    else if (anchorKey) {
      const restored = [...body.querySelectorAll<HTMLElement>('[data-process-key]')]
        .find(node => node.dataset.processKey === anchorKey);
      if (restored) body.scrollTop += restored.getBoundingClientRect().top - body.getBoundingClientRect().top - anchorOffset;
    }
    restoredScrollTop = body.scrollTop;
    restorePageAnchor();
  } catch {
    if (!controller.signal.aborted) {
      if (initial) error.value = true;
      else pageError.value = true;
      pageRetryAfter = Date.now() + 1500;
    }
  } finally {
    if (loadAbort === controller) {
      loadAbort = null;
      loading.value = false;
      loadingMore.value = false;
      if (!pageError.value && !error.value) fillShortPage();
      if (!pageError.value && !error.value && open.value && loaded.value &&
          completeProcessRefresh(refreshGate, !hasNewerItems.value)) {
        void loadPage('refresh');
      }
    }
  }
};

const load = (): Promise<void> => loadPage('initial');
const handleProcessScroll = (): void => {
  const body = processBodyRef.value;
  if (body) followLatest = body.scrollHeight - body.clientHeight - body.scrollTop <= 72;
  if (!body || loadingMore.value || Date.now() < pageRetryAfter) return;
  if (Math.abs(body.scrollTop - restoredScrollTop) > 2) pageAnchor = null;
  fillBudget = 4;
  if (body.scrollTop <= 96 && hasOlderItems.value) void loadPage('older');
  else if (body.scrollHeight - body.clientHeight - body.scrollTop <= 96 && hasNewerItems.value) void loadPage('newer');
};
// A page containing only filtered items may not create a scrollbar. A wheel
// gesture must still be able to continue reading it without a load button.
const handleProcessWheel = (event: WheelEvent): void => {
  pageAnchor = null;
  if (event.deltaY < 0) followLatest = false;
  const body = processBodyRef.value;
  if (!body || body.scrollHeight > body.clientHeight + 2 || Date.now() < pageRetryAfter) return;
  fillBudget = 4;
  void loadPage(event.deltaY < 0 ? 'older' : 'newer');
};

const resetState = (): void => {
  clearReloadTimer();
  refreshGate.pending = false;
  loadAbort?.abort();
  loadAbort = null;
  open.value = false;
  loading.value = false;
  error.value = false;
  loaded.value = false;
  rows.value = [];
  pageError.value = false;
  pageRetryAfter = 0;
  fillBudget = 4;
  if (fillFrame !== null) window.cancelAnimationFrame(fillFrame);
  fillFrame = null;
  if (anchorFrame !== null) window.cancelAnimationFrame(anchorFrame);
  anchorFrame = null;
  if (liveFrame !== null) window.cancelAnimationFrame(liveFrame);
  liveFrame = null;
  followLatest = true;
  pageAnchor = null;
  processItems.value = [];
  hasOlderItems.value = false;
  hasNewerItems.value = false;
  firstItemIndex.value = null;
  lastItemIndex.value = null;
  openKeys.value = new Set();
  fallbackReasoningKey.value = 'projected:0';
  currentReasoningMatchKey.value = '';
  currentReasoningMatchIdentity.value = '';
  processBodyRef.value = null;
  patchViews.clear();
  structuredViews.clear();
};

// 轮次身份变化（虚拟列表行复用）即复位；轮次仍在运行时步数会增长，展开态跟随刷新。
watch(() => [props.sessionId, props.turnId], resetState);

// Some legacy row sources do not yet carry `reasoning_item_id`. Their current
// preview still represents one model round; advance a local identity when its
// text clears and starts again so a later thought is appended, never recycled.
watch(
  () => [currentReasoningItemId.value, reasoningText.value] as const,
  ([itemId, text], [previousItemId, previousText]) => {
    if (itemId || !text || previousItemId || !previousText) return;
    const sequence = Number(fallbackReasoningKey.value.split(':')[1] || '0') + 1;
    fallbackReasoningKey.value = `projected:${sequence}`;
  }
);

/**
 * The lazy log is deliberately not refetched for every reasoning tail.  Once
 * its current row exists, replace only that shallow row's entry in place; Vue
 * keeps all other keyed process DOM stable and the live text cannot jump to a
 * completed entry above it.
 */
const syncCurrentReasoningRow = (): void => {
  if (!loaded.value) return;
  bindCurrentReasoningRow();
  let changed = false;
  // A subsequent model request starts a different item. Mark the preceding
  // thought final immediately, before the debounced log refresh supplies its
  // durable status, so an old row never keeps the live spinner.
  for (let index = 0; index < rows.value.length; index += 1) {
    const row = rows.value[index];
    if (row.kind !== 'reasoning' || !row.entry.streaming || row.key === (currentReasoningMatchKey.value || currentReasoningRowKey.value)) continue;
    rows.value[index] = {
      kind: 'reasoning', key: row.key,
      entry: { ...row.entry, streaming: false }
    };
    changed = true;
  }
  if (reasoningText.value) {
    const targetKey = currentReasoningMatchKey.value || currentReasoningRowKey.value;
    const index = rows.value.findIndex((row) => row.kind === 'reasoning' && row.key === targetKey);
    const current = index >= 0 ? rows.value[index] : null;
    if (current?.kind === 'reasoning') {
      const next = { ...reasoningEntry.value };
      if (!next.streaming && current.entry.text.length > next.text.length) {
        next.text = current.entry.text;
        next.summary = reasoningPreview(next.text);
      }
      if (current.entry.text !== next.text || current.entry.streaming !== next.streaming) {
        rows.value[index] = { kind: 'reasoning', key: current.key, entry: { ...next, key: current.key } };
        changed = true;
      }
    }
  }
  if (changed) triggerRef(rows);
};
watch(
  () => [currentReasoningItemId.value, reasoningText.value, Boolean(props.reasoningStreaming), loaded.value],
  syncCurrentReasoningRow
);

// The final-answer identity can be corrected after a streamed assistant item
// gains tool_calls. Reclassify the already loaded durable rows immediately:
// waiting for the debounced page refresh left that item's prose hidden for the
// whole tool run, then made it pop back into place on completion.
watch(
  () => props.finalItemId,
  () => {
    if (!open.value || !loaded.value) return;
    rebuildRows();
    restorePageAnchor();
  }
);

// Debounce structural/phase changes; individual text deltas patch only their
// live row. A phase ending also reloads the complete durable reasoning text.
watch(
  () => [props.count, props.finalItemId, props.status, Boolean(props.reasoningStreaming)],
  () => {
    refreshGate.pending = true;
    if (!open.value || (!loaded.value && !loadAbort)) return;
    clearReloadTimer();
    reloadTimer = window.setTimeout(() => {
      reloadTimer = null;
      if (requestProcessRefresh(refreshGate, open.value && loaded.value && !loadAbort && !hasNewerItems.value)) {
        void loadPage('refresh');
      }
    }, 600);
  }
);
onBeforeUnmount(() => {
  if (liveFrame !== null) window.cancelAnimationFrame(liveFrame);
  if (anchorFrame !== null) window.cancelAnimationFrame(anchorFrame);
  if (fillFrame !== null) window.cancelAnimationFrame(fillFrame);
  clearReloadTimer();
  refreshGate.pending = false;
  loadAbort?.abort();
});

// ---------------------------------------------------------------- 条目装配

const TERMINAL_TOOL_STATUS = new Set(['completed', 'failed', 'cancelled', 'interrupted']);
const SKIP_KINDS = new Set(['user_message', 'tool_message']);

const asObject = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};

const safeJsonParse = (text: string): unknown => {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
};

/** ThreadLog 条目封装：payload 里可能再嵌一层原始事件体，摊平成一份数据。 */
const flattenItemPayload = (payload: unknown): Record<string, unknown> => {
  const outer = asObject(payload);
  const inner = asObject(outer.payload);
  const nestedData = { ...asObject(outer.data), ...asObject(inner.data) };
  if (Object.keys(inner).length === 0 && Object.keys(nestedData).length === 0) return outer;
  const merged: Record<string, unknown> = { ...outer, ...inner, ...nestedData };
  delete merged.payload;
  return merged;
};

const pickText = (source: Record<string, unknown>, keys: string[]): string => {
  for (const key of keys) {
    const value = source[key];
    if (typeof value === 'string' && value.trim()) return value.trim();
  }
  return '';
};

const stringifyDetail = (value: unknown): string => {
  try {
    const text = JSON.stringify(value ?? {});
    return text.length > 131072 ? `${text.slice(0, 131072)}…` : text;
  } catch {
    return '';
  }
};

const isCompactionSystemItem = (item: Record<string, unknown>): boolean => {
  if (String(item?.kind || '') !== 'system_message') return false;
  const data = flattenItemPayload(item?.payload);
  const meta = asObject(data.meta);
  const type = String(meta.type ?? data.type ?? '').trim().toLowerCase();
  const id = String(meta.compaction_id ?? meta.compactionId ?? data.compaction_id ?? data.compactionId ?? '').trim();
  return type === 'compaction_summary' || Boolean(id);
};

type PersistedToolCallIdentity = { name: string; functionName: string; arguments?: unknown };

const collectAssistantToolCalls = (
  items: Array<Record<string, unknown>>
): Map<string, PersistedToolCallIdentity> => {
  const identities = new Map<string, PersistedToolCallIdentity>();
  for (const item of items) {
    if (String(item?.kind || '') !== 'assistant_message') continue;
    const data = flattenItemPayload(item?.payload);
    const calls = data.tool_calls ?? data.toolCalls;
    if (!Array.isArray(calls)) continue;
    for (const rawCall of calls) {
      const call = asObject(rawCall);
      if (!call) continue;
      const functionPayload = Object.keys(asObject(call.function)).length > 0
        ? asObject(call.function)
        : call;
      const callId = pickText(call, ['id', 'tool_call_id', 'toolCallId', 'call_id', 'callId']);
      const name = pickText(functionPayload, ['name', 'tool', 'tool_name', 'toolName']) ||
        pickText(call, ['name', 'tool', 'tool_name', 'toolName']);
      if (!callId || !name) continue;
      identities.set(callId, {
        name,
        functionName: pickText(functionPayload, ['name', 'function_name', 'functionName']) || name,
        arguments: functionPayload.arguments ?? call.arguments
      });
    }
  }
  return identities;
};

/**
 * 持久条目 → WorkflowItem：字段口径与运行时 reducer 的投影条目一致
 * （eventType / tool 身份 / call 引用 / detail），让 run 折叠与条目渲染
 * 直接复用 toolTimelineModel 的既有链路。
 */
const mapWorkflowItem = (
  item: Record<string, unknown>,
  persistedToolCalls: ReadonlyMap<string, PersistedToolCallIdentity>
): WorkflowItem | null => {
  const kind = String(item?.kind || '');
  if (!kind || kind === 'assistant_message' || SKIP_KINDS.has(kind) ||
    (kind === 'system_message' && !isCompactionSystemItem(item))) return null;
  const data = flattenItemPayload(item?.payload);
  const status = String(item?.status || data.status || 'completed');
  const eventType =
    kind === 'system_message'
      ? 'compaction'
      : kind === 'tool_call'
      ? TERMINAL_TOOL_STATUS.has(status)
        ? 'tool_result'
        : 'tool_call'
      : String(data.event_type || data.eventType || kind);
  const mapped: WorkflowItem = {
    id: String(item?.item_id || ''),
    eventType,
    status,
    isTool: true,
    detail: stringifyDetail(data),
    updatedSeq: Number(item?.revision) || undefined
  };
  if (kind === 'system_message') {
    mapped.toolName = 'context_compaction';
    mapped.tool = 'context_compaction';
    const summary = pickText(data, [
      'summary_text', 'summaryText', 'summary_model_output', 'summaryModelOutput',
      // Automatic compaction replacement rows persist the bounded summary in
      // the system item's content field.
      'content'
    ]);
    if (summary) {
      mapped.detail = stringifyDetail({ ...data, summary_text: summary });
    }
  }
  const callId = pickText(data, ['tool_call_id', 'toolCallId', 'call_id', 'callId']);
  const persisted = callId ? persistedToolCalls.get(callId) : undefined;
  const toolName = pickText(data, ['tool', 'name', 'tool_name', 'toolName']) || persisted?.name || '';
  if (toolName) {
    mapped.toolName = toolName;
    mapped.tool = toolName;
  }
  const displayName = pickText(data, ['tool_display_name', 'toolDisplayName', 'display_name', 'displayName']);
  if (displayName) {
    mapped.toolDisplayName = displayName;
    mapped.tool_display_name = displayName;
  }
  const runtimeName = pickText(data, ['tool_runtime_name', 'toolRuntimeName', 'runtime_name', 'runtimeName']);
  if (runtimeName) {
    mapped.toolRuntimeName = runtimeName;
    mapped.tool_runtime_name = runtimeName;
  }
  const functionName = pickText(data, ['tool_function_name', 'toolFunctionName', 'function_name', 'functionName']) || persisted?.functionName || '';
  if (functionName) {
    mapped.toolFunctionName = functionName;
    mapped.tool_function_name = functionName;
  }
  if (callId) {
    mapped.toolCallId = callId;
    mapped.tool_call_id = callId;
  }
  const commandSessionId = pickText(data, ['command_session_id', 'commandSessionId']);
  if (commandSessionId) {
    mapped.commandSessionId = commandSessionId;
    mapped.command_session_id = commandSessionId;
  }
  // 补丁/编辑类条目需要原始调用参数（与 reducer 的 buildProjectedToolCallRawDetail 同口径）。
  const nestedFunction = asObject(data.function);
  const args = data.args ?? data.arguments ?? data.input ?? nestedFunction.arguments ?? persisted?.arguments;
  if (args !== undefined && (eventType === 'tool_call' || eventType === 'tool_result')) {
    const parsed = typeof args === 'string' ? safeJsonParse(args) : args;
    const rawCall = stringifyDetail({ tool: toolName || functionName || runtimeName, arguments: parsed });
    if (rawCall) {
      mapped.toolCallRawDetail = rawCall;
      mapped.tool_call_raw_detail = rawCall;
    }
  }
  return mapped;
};

const buildRows = (items: Array<Record<string, unknown>>): ProcessRow[] => {
  const rows: ProcessRow[] = [];
  if (!Array.isArray(items) || items.length === 0) return rows;
  // 最后一条助手消息的正文就是正式回复（由正文区渲染），不进过程区。
  // The view row owns the terminal answer identity. Inferring it from the
  // currently loaded page made an intermediate model print disappear whenever
  // the page was truncated or a late item arrived.
  const lastAssistantId = String(props.finalItemId || '');
  type Slot = { type: 'message'; item: Record<string, unknown> } | { type: 'tool'; item: Record<string, unknown> };
  const slots: Slot[] = [];
  const mappedItems: WorkflowItem[] = [];
  const persistedToolCalls = collectAssistantToolCalls(items);
  for (const item of items) {
    const kind = String(item?.kind || '');
    if (SKIP_KINDS.has(kind) || (kind === 'system_message' && !isCompactionSystemItem(item))) continue;
    if (kind === 'assistant_message') {
      slots.push({ type: 'message', item });
      continue;
    }
    const mapped = mapWorkflowItem(item, persistedToolCalls);
    if (!mapped) continue;
    // A lifecycle row can precede the decorated tool result during replay.
    // Keep the process timeline quiet until its identity is known; otherwise
    // the renderer turns the incomplete row into a misleading “unknown tool”.
    if (!String(mapped.toolName || mapped.tool || mapped.tool_name || '').trim()) continue;
    slots.push({ type: 'tool', item });
    mappedItems.push(mapped);
  }
  if (!slots.length) return rows;
  // The process endpoint can overlap a durable assistant row with the live
  // hand-off row while a model round is being committed.  Those rows have
  // different ids but represent one visible identity.  Collapse by execution
  // turn and model round before creating reasoning entries; otherwise the chat
  // area shows the same thinking step twice.
  const selectedMessages = new Map<string, Slot>();
  const messageIdentity = (item: Record<string, unknown>): string => {
    const payload = flattenItemPayload(item.payload);
    const turnId = String(item.turn_id || payload.turn_id || '');
    const round = Number(item.model_round ?? payload.model_round ?? 0);
    return `${turnId}\u0000${Number.isFinite(round) ? round : 0}`;
  };
  const messageScore = (item: Record<string, unknown>): number => {
    const payload = flattenItemPayload(item.payload);
    return String(payload.content || '').length + String(payload.reasoning || payload.reasoning_content || '').length;
  };
  for (const slot of slots) {
    if (slot.type !== 'message') continue;
    const identity = messageIdentity(slot.item);
    const current = selectedMessages.get(identity);
    if (!current || messageScore(slot.item) > messageScore(current.item) ||
      (messageScore(slot.item) === messageScore(current.item) &&
       String(slot.item.item_id || '').endsWith(`:text-${String(slot.item.model_round ?? flattenItemPayload(slot.item.payload).model_round ?? 0)}`))) {
      selectedMessages.set(identity, slot);
    }
  }
  const dedupedSlots = slots.filter((slot) => {
    if (slot.type !== 'message') return true;
    const identity = messageIdentity(slot.item);
    return selectedMessages.get(identity) === slot;
  });
  // run 折叠（call/output/result 配对）复用既有链路；有界窗口与折叠提示同投影语义。
  const runs = buildWorkflowToolRuns(mappedItems);
  const entryByItemId = new Map<string, TimelineToolEntry>();
  for (const run of runs) {
    const entry = buildTimelineToolEntry(run, t);
    entryByItemId.set(String(run.key), entry);
    for (const item of [run.callItem, run.outputItem, run.resultItem]) {
      if (item) entryByItemId.set(String(item.id ?? ''), entry);
    }
  }
  const emittedTools = new Set<string>();
  for (const slot of dedupedSlots) {
    if (slot.type === 'tool') {
      const entry = entryByItemId.get(String(slot.item?.item_id || ''));
      if (entry && !emittedTools.has(entry.key)) {
        emittedTools.add(entry.key);
        rows.push({ kind: 'tool', key: `tool:${entry.key}`, entry });
      }
      continue;
    }
    const payload = flattenItemPayload(slot.item?.payload);
    const itemId = String(slot.item?.item_id || '');
    const persistedReasoning = typeof payload.reasoning === 'string'
      ? payload.reasoning.trim()
      : typeof payload.reasoning_content === 'string' ? payload.reasoning_content.trim() : '';
    // The bounded display row carries only the newest reasoning preview. Merge
    // it into its durable assistant item so it stays at the right point in the
    // timeline instead of being pinned above all process records.
    const isCurrentReasoning = itemId === currentReasoningItemId.value;
    // The thread-view row contains the newest bounded preview. Prefer it for
    // its matching durable item so the active entry updates without a reload.
    const reasoning = (isCurrentReasoning && props.reasoningStreaming ? reasoningText.value : '') || persistedReasoning ||
      (isCurrentReasoning ? reasoningText.value : '');
    if (reasoning) {
      const key = `reasoning:${itemId}`;
      rows.push({
        kind: 'reasoning',
        key,
        modelRound: Number.isSafeInteger(Number(payload.model_round)) ? Number(payload.model_round) : undefined,
        entry: {
          kind: 'reasoning', key,
          streaming: isCurrentReasoning && Boolean(props.reasoningStreaming),
          summary: reasoningPreview(reasoning), text: reasoning
        }
      });
    }
    const content = typeof payload.content === 'string' ? payload.content.trim() : '';
    if (content && itemId !== lastAssistantId) {
      rows.push({ kind: 'print', key: `print:${itemId}`, text: content });
    }
  }
  return rows;
};
</script>

<style scoped>
/* 过程条与 MessageTimelineBlocks 的 .timeline-process-head 同一形态（样式为 scoped，各自持有）。 */
/* 卡片容器：收起时是浅暖底圆角胶囊，展开后是白底细边卡片，与气泡正文明确分区。 */
.timeline-turn-process {
  width: 100%;
  min-width: 0;
  background: transparent;
}

.timeline-turn-process.is-open {
  background: transparent;
}

.timeline-process-head {
  display: flex;
  align-items: center;
  gap: 8px;
  box-sizing: border-box;
  width: 100%;
  height: 40px;
  padding: 0 24px;
  border: 0;
  border-radius: 0;
  background: transparent;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: calc(14px * var(--messenger-font-scale, 1));
  text-align: left;
  cursor: pointer;
}

/* The work-process row is an unobtrusive disclosure, not a selected list row.
   Hover feedback lives on the chevron button so scanning the timeline remains calm. */
.timeline-process-head:hover {
  background: transparent;
}

.timeline-process-head:focus-visible {
  outline: none;
}

.timeline-process-head:focus-visible .timeline-process-gutter::before {
  border-color: rgba(var(--ui-accent-rgb), 0.48);
  box-shadow: 0 0 0 2px rgba(var(--ui-accent-rgb), 0.14);
}

.timeline-process-gutter {
  position: relative;
  flex: 0 0 auto;
  align-self: stretch;
  width: 26px;
}

.timeline-process-gutter::before {
  content: '';
  position: absolute;
  top: 8px;
  bottom: 8px;
  left: 1px;
  width: 24px;
  box-sizing: border-box;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 10px;
  background: var(--mz-surface, #ffffff);
  box-shadow: 0 1px 2px rgba(31, 35, 41, 0.06);
  transition: border-color 0.16s ease, box-shadow 0.16s ease;
}

.timeline-process-gutter:hover::before {
  border-color: var(--mz-border-strong, #d8d5d0);
  box-shadow: 0 2px 5px rgba(31, 35, 41, 0.08);
}

.timeline-process-arrow {
  position: absolute;
  z-index: 1;
  top: 50%;
  left: 5px;
  width: 16px;
  margin-top: -8px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 14px;
  line-height: 16px;
  text-align: center;
  transition: transform 0.16s ease;
}

.timeline-process-arrow.is-open {
  transform: rotate(90deg);
}

.timeline-process-title {
  flex: 0 0 auto;
  font-weight: 500;
}

.timeline-process-count {
  flex: 0 0 auto;
  padding: 0;
  border-radius: 0;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 13px;
  font-weight: 400;
  line-height: inherit;
  white-space: nowrap;
}

.timeline-process-latest {
  flex: 1 1 auto;
  min-width: 0;
  color: var(--mz-text-muted, #8a8f99);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 过程体限高滚动：一次工作过程固定在同一区域内展开，长轮次不撑开整条消息。 */
.timeline-process-body {
  width: 100%;
  min-width: 0;
  max-height: min(420px, 48vh);
  overflow-y: auto;
  overscroll-behavior: contain;
  overflow-anchor: none;
}

.timeline-process-note {
  padding: 4px 24px;
  font-size: 12px;
  color: var(--mz-text-muted, #8a8f99);
}

.timeline-process-retry {
  margin: 0 24px 4px;
  padding: 2px 10px;
  border: 1px solid var(--border-color, #ddd);
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  cursor: pointer;
}

.timeline-process-page-state {
  display: block;
  max-width: calc(100% - 48px);
  margin: 8px 24px 12px 52px;
  padding: 5px 10px;
  border: 1px solid rgba(100, 116, 139, 0.22);
  border-radius: 6px;
  background: var(--mz-surface, #fff);
  color: var(--mz-text-secondary, #64748b);
  font-size: 12px;
  cursor: default;
}

.timeline-process-page-state:hover {
  border-color: rgba(59, 130, 246, 0.38);
  color: var(--mz-text, #1f2937);
}

.timeline-process-page-state[aria-busy="true"] {
  cursor: wait;
  opacity: 0.65;
}

/* 中间打印块度量与投影正文块一致：全宽、左右 24px、上下 6px。 */
.timeline-body-block {
  position: relative;
  width: 100%;
  min-width: 0;
  padding: 6px 24px;
  color: var(--mz-text, #1f2329);
  font-size: calc(14px * var(--messenger-font-scale, 1));
  line-height: 1.7;
}

/* 流式实时行：与正文块同度量，色度略降 + 尾部闪烁光标，提示内容仍在生成。 */
.timeline-live-print {
  color: var(--mz-text-secondary, #3d3d3d);
}

.timeline-live-print::after {
  content: '';
  display: inline-block;
  width: 2px;
  height: 1em;
  margin-left: 2px;
  vertical-align: -0.15em;
  background: var(--mz-primary, #c96443);
  animation: timeline-live-print-caret 0.9s steps(1) infinite;
}

@keyframes timeline-live-print-caret {
  50% {
    opacity: 0;
  }
}
</style>
