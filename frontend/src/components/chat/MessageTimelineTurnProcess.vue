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
        <span v-if="open" class="timeline-process-gutter-line"></span>
      </span>
      <span class="timeline-process-title">{{ t('chat.timeline.workProcess') }}</span>
      <span class="timeline-process-count">{{ t('chat.timeline.workProcessSteps', { count }) }}</span>
      <span v-if="!open && reasoningSummary" class="timeline-process-latest" :title="reasoningSummary">
        {{ reasoningSummary }}
      </span>
      <span v-if="!open && failedCount > 0" class="timeline-process-latest">
        {{ t('chat.timeline.workProcessFailed', { count: failedCount }) }}
      </span>
    </button>

    <div v-if="open" class="timeline-process-body">
      <MessageTimelineThinkingEntry
        v-if="reasoningText && (!loaded || !rows.some((row) => row.kind === 'reasoning' && row.key === `reasoning:${reasoningItemId}`))"
        :entry="reasoningEntry"
        :open="openKeys.has('reasoning:projected')"
        @toggle="toggleKey('reasoning:projected')"
      />
      <div v-if="loading && !loaded" class="timeline-process-note">{{ t('chat.timeline.processLoading') }}</div>
      <template v-else-if="error">
        <div class="timeline-process-note">{{ t('chat.timeline.processFailed') }}</div>
        <button class="timeline-process-retry" type="button" @click="load">{{ t('chat.timeline.processRetry') }}</button>
      </template>
      <template v-else>
        <div class="timeline-group">
        <template v-for="row in rows" :key="row.key">
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
            />
          </div>
        </template>
        </div>
        <button
          v-if="hasMoreItems || omittedRuns > 0"
          class="timeline-process-more"
          type="button"
          :disabled="loadingMore || !hasMoreItems"
          @click="loadMore"
        >
          {{ loadingMore ? t('chat.timeline.processLoading') : hasMoreItems ? t('chat.timeline.processLoadEarlier') : t('chat.timeline.omittedRuns', { count: omittedRuns }) }}
        </button>
        <!-- 有工具轮次的流式尾部：正文区在流式期间让位，活动文本在这里直播，
             轮次完成后由正文区接管（buildRows 会排除最后一条助手正文，不重复）。 -->
        <div v-if="liveTail" class="timeline-body-block timeline-live-print">{{ liveTail }}</div>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue';

import MessageMarkdownBody from './MessageMarkdownBody.vue';
import MessageTimelineThinkingEntry from './MessageTimelineThinkingEntry.vue';
import MessageTimelineToolEntry from './MessageTimelineToolEntry.vue';
import { t } from '@/i18n';
import { getThreadLogTurn } from '@/api/chat';
import {
  buildTimelineToolEntry,
  TIMELINE_ENTRY_RENDER_LIMIT,
  type TimelineReasoningEntry,
  type TimelineToolEntry
} from './toolTimelineModel';
import { buildWorkflowToolRuns, type WorkflowItem } from './toolWorkflowRunModel';
import { buildTimelinePatchView } from './toolTimelinePatch';
import { buildTimelineStructuredView } from './toolTimelineStructured';
import type { ToolWorkflowPatchView, ToolWorkflowStructuredView } from './toolWorkflowTypes';

type ProcessRow =
  | { kind: 'reasoning'; key: string; entry: TimelineReasoningEntry }
  | { kind: 'tool'; key: string; entry: TimelineToolEntry }
  | { kind: 'print'; key: string; text: string };

const props = defineProps<{
  sessionId: string;
  turnId: string;
  count: number;
  failedCount?: number;
  /** 流式期间的活动文本尾部：正文区让位时由过程区实时行承接展示。 */
  liveTail?: string;
  reasoning?: string;
  reasoningItemId?: string;
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
const omittedRuns = ref(0);
const hasMoreItems = ref(false);
const loadingMore = ref(false);
const nextAfter = ref(-1);
const loadMoreItems = ref<Array<Record<string, unknown>>>([]);
const visibleRunLimit = ref(TIMELINE_ENTRY_RENDER_LIMIT);
const openKeys = ref(new Set<string>());
const patchViews = new Map<string, ToolWorkflowPatchView | null>();
const structuredViews = new Map<string, ToolWorkflowStructuredView | null>();
let loadAbort: AbortController | null = null;
let reloadTimer: number | null = null;

const cacheKeyPrefix = computed(() => `turn-process:${props.sessionId}:${props.turnId}:`);
const reasoningText = computed(() => String(props.reasoning || '').trim());
const reasoningSummary = computed(() => firstLine(reasoningText.value));
const firstLine = (text: string): string => {
  const normalized = text.replace(/\s+/g, ' ').trim();
  return normalized.length > 160 ? `${normalized.slice(0, 160)}…` : normalized;
};
const reasoningEntry = computed<TimelineReasoningEntry>(() => ({
  kind: 'reasoning', key: 'reasoning:projected', streaming: Boolean(props.reasoning && !loaded.value),
  summary: firstLine(reasoningText.value), text: reasoningText.value
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

const load = async (): Promise<void> => {
  const sessionId = props.sessionId.trim();
  const turnId = props.turnId.trim();
  if (!sessionId || !turnId) return;
  const controller = new AbortController();
  loadAbort?.abort();
  loadAbort = controller;
  loading.value = true;
  error.value = false;
  try {
    // 展开时只读一个有界窗口；较大的轮次通过“加载更早过程”继续向前
    // 拉取，避免一次把历史测试日志复制进 DOM。
    const items: Array<Record<string, unknown>> = [];
    let after = -1;
    let more = false;
    for (let page = 0; page < 4; page += 1) {
      const response = await getThreadLogTurn(
        sessionId,
        turnId,
        { item_after: after, limit: 100 },
        { signal: controller.signal }
      );
      if (controller.signal.aborted) return;
      const turn = response.data?.data?.turn;
      const batch = Array.isArray(turn?.items) ? turn.items : [];
      items.push(...batch);
      more = Boolean(turn?.has_more);
      if (!more || batch.length === 0) break;
      const next = Number(turn.next_after);
      if (!Number.isSafeInteger(next) || next <= after) {
        more = false;
        break;
      }
      after = next;
    }
    if (controller.signal.aborted) return;
    loadMoreItems.value = items;
    visibleRunLimit.value = TIMELINE_ENTRY_RENDER_LIMIT;
    rows.value = buildRows(items);
    nextAfter.value = after;
    hasMoreItems.value = more;
    loaded.value = true;
  } catch {
    if (!controller.signal.aborted) error.value = true;
  } finally {
    if (loadAbort === controller) {
      loadAbort = null;
      loading.value = false;
    }
  }
};

const loadMore = async (): Promise<void> => {
  if (!hasMoreItems.value || loadingMore.value || loadAbort || !props.sessionId || !props.turnId) return;
  // Preserve chronological order: reveal the next bounded set already in
  // memory before asking the server for a later item page.
  if (omittedRuns.value > 0) {
    visibleRunLimit.value += TIMELINE_ENTRY_RENDER_LIMIT;
    rows.value = buildRows(loadMoreItems.value);
    return;
  }
  const controller = new AbortController();
  loadAbort = controller;
  loadingMore.value = true;
  try {
    const response = await getThreadLogTurn(props.sessionId, props.turnId,
      { item_after: nextAfter.value, limit: 100 }, { signal: controller.signal });
    if (controller.signal.aborted) return;
    const turn = response.data?.data?.turn;
    const batch = Array.isArray(turn?.items) ? turn.items : [];
    if (!batch.length) { hasMoreItems.value = false; return; }
    // The API is forward-paged. Keep a bounded client window so repeatedly
    // expanding a pathological turn cannot grow memory without limit.
    const mergedItems = loadMoreItems.value.concat(batch).slice(-10000);
    loadMoreItems.value = mergedItems;
    visibleRunLimit.value += TIMELINE_ENTRY_RENDER_LIMIT;
    rows.value = buildRows(mergedItems);
    const next = Number(turn.next_after);
    hasMoreItems.value = Boolean(turn.has_more && Number.isSafeInteger(next) && next > nextAfter.value);
    if (Number.isSafeInteger(next) && next > nextAfter.value) nextAfter.value = next;
  } catch {
    // The already-rendered process remains usable; retrying the button is safe.
  } finally {
    if (loadAbort === controller) loadAbort = null;
    loadingMore.value = false;
  }
};

const resetState = (): void => {
  clearReloadTimer();
  loadAbort?.abort();
  loadAbort = null;
  open.value = false;
  loading.value = false;
  error.value = false;
  loaded.value = false;
  rows.value = [];
  loadMoreItems.value = [];
  visibleRunLimit.value = TIMELINE_ENTRY_RENDER_LIMIT;
  hasMoreItems.value = false;
  nextAfter.value = -1;
  omittedRuns.value = 0;
  openKeys.value = new Set();
  patchViews.clear();
  structuredViews.clear();
};

// 轮次身份变化（虚拟列表行复用）即复位；轮次仍在运行时步数会增长，展开态跟随刷新。
watch(() => [props.sessionId, props.turnId], resetState);
// 流式期间步数高频增长：防抖跟随刷新（尾部触发），避免每次步数变化都整段重拉造成抖动。
watch(
  () => props.count,
  () => {
    if (!open.value || !loaded.value) return;
    clearReloadTimer();
    reloadTimer = window.setTimeout(() => {
      reloadTimer = null;
      if (open.value) void load();
    }, 600);
  }
);
onBeforeUnmount(() => {
  clearReloadTimer();
  loadAbort?.abort();
});

// ---------------------------------------------------------------- 条目装配

const TERMINAL_TOOL_STATUS = new Set(['completed', 'failed', 'cancelled', 'interrupted']);
const SKIP_KINDS = new Set(['user_message', 'tool_message', 'system_message']);

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
  if (Object.keys(inner).length === 0) return outer;
  const merged: Record<string, unknown> = { ...outer };
  delete merged.payload;
  return { ...merged, ...inner };
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

/**
 * 持久条目 → WorkflowItem：字段口径与运行时 reducer 的投影条目一致
 * （eventType / tool 身份 / call 引用 / detail），让 run 折叠与条目渲染
 * 直接复用 toolTimelineModel 的既有链路。
 */
const mapWorkflowItem = (item: Record<string, unknown>): WorkflowItem | null => {
  const kind = String(item?.kind || '');
  if (!kind || kind === 'assistant_message' || SKIP_KINDS.has(kind)) return null;
  const data = flattenItemPayload(item?.payload);
  const status = String(item?.status || data.status || 'completed');
  const eventType =
    kind === 'tool_call'
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
  const toolName = pickText(data, ['tool', 'name', 'tool_name', 'toolName']);
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
  const functionName = pickText(data, ['tool_function_name', 'toolFunctionName', 'function_name', 'functionName']);
  if (functionName) {
    mapped.toolFunctionName = functionName;
    mapped.tool_function_name = functionName;
  }
  const callId = pickText(data, ['tool_call_id', 'toolCallId', 'call_id', 'callId']);
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
  const args = data.args ?? data.arguments ?? data.input ?? nestedFunction.arguments;
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
  for (const item of items) {
    const kind = String(item?.kind || '');
    if (SKIP_KINDS.has(kind)) continue;
    if (kind === 'assistant_message') {
      slots.push({ type: 'message', item });
      continue;
    }
    const mapped = mapWorkflowItem(item);
    if (!mapped) continue;
    slots.push({ type: 'tool', item });
    mappedItems.push(mapped);
  }
  if (!slots.length) return rows;
  // run 折叠（call/output/result 配对）复用既有链路；有界窗口与折叠提示同投影语义。
  const runs = buildWorkflowToolRuns(mappedItems);
  omittedRuns.value = Math.max(runs.length - visibleRunLimit.value, 0);
  const visibleRuns = runs.slice(0, visibleRunLimit.value);
  const entryByItemId = new Map<string, TimelineToolEntry>();
  for (const run of visibleRuns) {
    const entry = buildTimelineToolEntry(run, t);
    entryByItemId.set(String(run.key), entry);
    if (run.callItem) entryByItemId.set(String(run.callItem.id ?? ''), entry);
  }
  for (const slot of slots) {
    if (slot.type === 'tool') {
      const entry = entryByItemId.get(String(slot.item?.item_id || ''));
      if (entry) rows.push({ kind: 'tool', key: `tool:${entry.key}`, entry });
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
    const reasoning = persistedReasoning ||
      (itemId === String(props.reasoningItemId || '') ? reasoningText.value : '');
    if (reasoning) {
      rows.push({
        kind: 'reasoning',
        key: `reasoning:${itemId}`,
        entry: { kind: 'reasoning', key: `reasoning:${itemId}`, streaming: false, summary: firstLine(reasoning), text: reasoning }
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
  border: 1px solid transparent;
  border-radius: 10px;
  background: var(--mz-timeline-hover, #f6f5f3);
  overflow: hidden;
  transition: background-color 0.16s ease, border-color 0.16s ease, box-shadow 0.16s ease;
}

.timeline-turn-process.is-open {
  border-color: var(--mz-border, #e8e6e3);
  background: var(--mz-surface, #ffffff);
  box-shadow: 0 1px 2px rgba(31, 35, 41, 0.04);
}

.timeline-process-head {
  display: flex;
  align-items: center;
  gap: 8px;
  box-sizing: border-box;
  width: 100%;
  height: 34px;
  padding: 0 24px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: calc(13px * var(--messenger-font-scale, 1));
  text-align: left;
  cursor: pointer;
}

.timeline-process-head:hover {
  background: var(--mz-selected, #edeae6);
}

.timeline-turn-process.is-open .timeline-process-head:hover {
  background: var(--mz-timeline-hover, #f6f5f3);
}

.timeline-process-gutter {
  position: relative;
  flex: 0 0 auto;
  align-self: stretch;
  width: 20px;
}

.timeline-process-gutter-line {
  position: absolute;
  top: calc(50% + 7px);
  bottom: 0;
  left: 9.5px;
  width: 1px;
  background: var(--mz-timeline-line, #e2dfda);
}

.timeline-process-arrow {
  position: absolute;
  top: 50%;
  left: 4px;
  width: 12px;
  margin-top: -6px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  line-height: 12px;
  text-align: center;
  transition: transform 0.16s ease;
}

.timeline-process-arrow.is-open {
  transform: rotate(90deg);
}

.timeline-process-title {
  flex: 0 0 auto;
  font-weight: 600;
}

.timeline-process-count {
  flex: 0 0 auto;
  padding: 1px 7px;
  border-radius: 999px;
  background: var(--mz-selected, #edeae6);
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  font-weight: 400;
  line-height: 16px;
  white-space: nowrap;
}

/* 展开态徽章带一点主色，呼应卡片的「活动区」身份。 */
.timeline-turn-process.is-open .timeline-process-count {
  background: var(--mz-primary-tint, #f6e9e3);
  color: var(--mz-primary, #c96443);
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
  scrollbar-gutter: stable;
}

.timeline-process-body::-webkit-scrollbar {
  width: 8px;
}

.timeline-process-body::-webkit-scrollbar-thumb {
  border-radius: 4px;
  background: var(--mz-border, #e8e6e3);
}

.timeline-process-body::-webkit-scrollbar-track {
  background: transparent;
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

.timeline-process-more {
  display: block;
  max-width: calc(100% - 48px);
  margin: 8px 24px 12px 52px;
  padding: 5px 10px;
  border: 1px solid rgba(100, 116, 139, 0.22);
  border-radius: 6px;
  background: #fff;
  color: var(--mz-text-secondary, #64748b);
  font-size: 12px;
  cursor: pointer;
}

.timeline-process-more:hover:not(:disabled) {
  border-color: rgba(59, 130, 246, 0.38);
  color: var(--mz-text, #1f2937);
}

.timeline-process-more:disabled {
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
