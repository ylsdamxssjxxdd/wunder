<template>
  <template v-for="block in renderBlocks" :key="block.id">
    <div
      v-if="block.kind === 'activity'"
      class="timeline-group"
      :class="{ 'is-open': block.open }"
      data-turn-slot="activity"
      :data-activity-id="block.id"
    >
      <button
        class="timeline-group-head"
        type="button"
        :aria-expanded="block.open"
        @click="toggleActivity(block.id)"
      >
        <span class="timeline-group-gutter" aria-hidden="true">
          <i
            :class="['fa-solid', 'fa-caret-right', 'timeline-group-arrow', { 'is-open': block.open }]"
          ></i>
          <span v-if="block.open" class="timeline-group-gutter-line"></span>
        </span>
        <span class="timeline-group-title">{{ activityTitle(block) }}</span>
        <span v-if="!block.open && block.latestSummary" class="timeline-group-latest">{{ block.latestSummary }}</span>
      </button>

      <div v-if="block.open" class="timeline-group-body">
        <template v-for="row in block.rows" :key="row.key">
          <MessageTimelineThinkingEntry
            v-if="row.kind === 'reasoning'"
            :entry="row.entry"
            :open="reasoningOpenKeys.includes(row.key)"
            @toggle="toggleReasoning(row.key)"
          />
          <MessageTimelineToolEntry
            v-else
            :entry="row.entry"
            :open="entryOpenKeys.includes(row.key)"
            :patch-view="patchViewFor(row.entry)"
            @toggle="toggleEntry(row.key)"
          />
        </template>

        <button v-if="block.isLast && hiddenToolCount > 0" class="timeline-group-more" type="button" @click="showMoreEntries">
          {{ t('chat.timeline.showMoreEntries', { count: hiddenToolCount }) }}
        </button>

        <div v-if="block.isLast && omittedRuns > 0" class="timeline-group-note" role="note">
          {{ t('chat.timeline.omittedRuns', { count: omittedRuns }) }}
        </div>
      </div>
    </div>

    <div
      v-else
      class="timeline-body-block"
      :class="{ 'is-streaming': block.streaming }"
      data-turn-slot="body"
    >
      <MessageMarkdownBody
        :cache-key="`${cacheKeyPrefix}${block.id}`"
        :content="block.text"
        :message="message ?? undefined"
        :runtime-message-id="runtimeMessageId"
        :runtime-user-turn-id="runtimeUserTurnId"
        :runtime-model-turn-id="runtimeModelTurnId"
        :session-id="sessionId"
        :content-truncated="contentTruncated"
        :assistant-display="true"
        :explicit-content="true"
        :streaming="block.streaming"
        :throttle-ms="throttleMs"
        :resolve-workspace-path="resolveWorkspacePath"
        @rendered="emit('markdown-rendered', $event)"
        @history-message-hydrated="emit('history-hydrated', $event as Record<string, unknown>)"
      />
      <!-- 正文块尾部留给出调用方的附加内容（图片/音频附件等，仅最后一段）。 -->
      <slot name="body-tail" :block="block" :is-last="block.isLast" />
    </div>
  </template>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import MessageMarkdownBody from './MessageMarkdownBody.vue';
import MessageTimelineThinkingEntry from './MessageTimelineThinkingEntry.vue';
import MessageTimelineToolEntry from './MessageTimelineToolEntry.vue';
import {
  TIMELINE_ENTRY_PAGE_SIZE,
  TIMELINE_ENTRY_RENDER_LIMIT,
  buildTimelineToolEntries,
  type TimelineReasoningEntry,
  type TimelineToolEntry
} from './toolTimelineModel';
import { buildTimelinePatchView } from './toolTimelinePatch';
import { MAX_OPEN_ENTRIES_PER_TURN } from './timelineGroupState';
import type { ToolWorkflowPatchView } from './toolWorkflowTypes';
import type { WorkflowItem } from './toolWorkflowRunModel';
import type { ChatRuntimeTimelineBlock } from '@/realtime/chat/chatRuntimeTypes';
import { t } from '@/i18n';

/**
 * 蜂巢时间线的块渲染单元（§7.3 / §7.4，形态对齐桌面端
 * `frontend-slint/ui/timeline.slint` 的 `BodyBlock` / `FoldEntry` / `GroupBar`）：
 * 正文块（全宽、无气泡）与思考/工具批次按投影给定的到达顺序交错铺开。
 *
 * `MessageTimelineAssistant`（主时间线）与子智能体详情弹窗共用本组件，
 * 保证子线程运行过程与主时间线是同一套渲染形态。
 *
 * 数据契约：块列表与工作流投影由调用方供给；投影层按行对象**原地**改写，
 * 因此 `contentVersion`（内容时钟）变化时本组件整体重算，行内文本才不落后帧。
 * 渲染状态（批次开合、条目开合、分页窗口）是组件私有的纯渲染状态。
 */
const props = withDefaults(defineProps<{
  blocks: ChatRuntimeTimelineBlock[];
  workflowItems: WorkflowItem[];
  sessionId?: string;
  /** 身份变化时复位展开状态（虚拟列表复用行组件 / 弹窗切换运行）。 */
  identityKey?: string;
  /** 最后一批是否默认展开；主时间线按 MAX_OPEN_TURNS 下发。 */
  defaultOpen?: boolean;
  /** 最后一段正文是否仍在流式输出。 */
  streaming?: boolean;
  /** 投影重算时钟：投影行原地写、引用不变，必须显式订阅。 */
  contentVersion?: string | number;
  /** 正文文本回调（最后一段套失败提示等整轮信息）；缺省保持投影原样。 */
  bodyTextTransform?: (text: string, isLast: boolean) => string;
  cacheKeyPrefix?: string;
  message?: Record<string, unknown> | null;
  runtimeMessageId?: string;
  runtimeUserTurnId?: string;
  runtimeModelTurnId?: string;
  contentTruncated?: boolean;
  resolveWorkspacePath?: (rawPath: string, context?: string) => string;
  throttleMs?: number;
}>(), {
  sessionId: '',
  identityKey: '',
  defaultOpen: false,
  streaming: false,
  contentVersion: 0,
  bodyTextTransform: undefined,
  cacheKeyPrefix: '',
  message: null,
  runtimeMessageId: '',
  runtimeUserTurnId: '',
  runtimeModelTurnId: '',
  contentTruncated: false,
  resolveWorkspacePath: undefined,
  throttleMs: 120
});

const emit = defineEmits<{
  (event: 'markdown-rendered', detail: unknown): void;
  (event: 'history-hydrated', detail: Record<string, unknown>): void;
  /** 本轮是否存在工具批次（主时间线据此套用 MAX_OPEN_TURNS 上限）。 */
  (event: 'activity', active: boolean): void;
}>();

defineOptions({ inheritAttrs: false });

type RenderReasoningRow = { kind: 'reasoning'; key: string; entry: TimelineReasoningEntry };
type RenderToolRow = { kind: 'tool'; key: string; entry: TimelineToolEntry };
type RenderActivityRow = RenderReasoningRow | RenderToolRow;

type RenderBodyBlock = {
  kind: 'body';
  id: string;
  text: string;
  streaming: boolean;
  isLast: boolean;
};

type RenderActivityBlock = {
  kind: 'activity';
  id: string;
  open: boolean;
  rows: RenderActivityRow[];
  toolCount: number;
  latestSummary: string;
  isLast: boolean;
};

type RenderBlock = RenderBodyBlock | RenderActivityBlock;

// ------------------------------------------------------- 工具/思考条目

const toolEntryModel = computed(() => buildTimelineToolEntries(props.workflowItems, t));
const allToolEntries = computed(() => toolEntryModel.value.entries);
const omittedRuns = computed(() => toolEntryModel.value.omittedRuns);

const entryRenderLimit = ref(TIMELINE_ENTRY_RENDER_LIMIT);
const visibleToolEntries = computed<TimelineToolEntry[]>(() => {
  const entries = allToolEntries.value;
  const limit = Math.max(entryRenderLimit.value, 1);
  return entries.length > limit ? entries.slice(entries.length - limit) : entries;
});
const hiddenToolCount = computed(() => Math.max(allToolEntries.value.length - visibleToolEntries.value.length, 0));
const showMoreEntries = (): void => {
  entryRenderLimit.value = Math.min(entryRenderLimit.value + TIMELINE_ENTRY_PAGE_SIZE, 400);
};

/**
 * 工具条目 → 批次行：批次里的 `tool` 行按底层工作流记录 id 认领自己的条目
 * （一次调用的 call / output / result 三条记录都指向同一条目）。
 */
const toolEntryByItemId = computed(() => {
  const map = new Map<string, TimelineToolEntry>();
  visibleToolEntries.value.forEach((entry) => {
    const run = entry.run as unknown as Record<string, Record<string, unknown> | null> | undefined;
    [run?.callItem?.id, run?.outputItem?.id, run?.resultItem?.id].forEach((id) => {
      if (id) map.set(String(id), entry);
    });
  });
  return map;
});

const reasoningSummaryOf = (text: string): string => {
  const normalized = text.replace(/\s+/g, ' ').trim();
  return normalized.length > 160 ? `${normalized.slice(0, 160)}…` : normalized;
};

// ------------------------------------------------- 时间线块（正文/批次交错）

const renderBlocks = computed<RenderBlock[]>(() => {
  // 与数据侧同一张内容时钟：块里的正文是投影层组合好的最新文本，
  // 行对象上的副本可能落后一帧，不订阅时钟就会停在首帧。
  void props.contentVersion;
  const blocks = props.blocks;
  const lastBodyIndex = blocks.reduce((last, block, index) => (block.kind === 'body' ? index : last), -1);
  const lastActivityIndex = blocks.reduce((last, block, index) => (block.kind === 'activity' ? index : last), -1);
  const claimed = new Set<string>();
  return blocks.map((block, index): RenderBlock => {
    if (block.kind === 'body') {
      const isLastBlock = index === blocks.length - 1;
      return {
        kind: 'body',
        id: block.id,
        // 失败提示等整轮信息只挂在最后一段正文上（与旧单段形态同源）。
        text: props.bodyTextTransform
          ? props.bodyTextTransform(block.text, index === lastBodyIndex)
          : block.text,
        streaming: index === lastBodyIndex && props.streaming,
        isLast: isLastBlock
      };
    }
    // 思考行与工具行按投影给定的到达顺序铺开：一批里可以有多次思考、多次调用。
    const rows: RenderActivityRow[] = [];
    block.rows.forEach((row) => {
      if (row.type === 'reasoning') {
        const text = String(row.text || '');
        if (!text.trim()) return;
        const key = `${props.identityKey}:${block.id}:reasoning:${row.itemId}`;
        rows.push({
          kind: 'reasoning',
          key,
          entry: {
            kind: 'reasoning',
            key,
            // 思考条目的流式状态以条目自身 status 为准（投影层下发）；
            // 旧行没有条目级状态时退回整轮 flag，保持既有表现。
            streaming: row.streaming === undefined
              ? index === lastActivityIndex && props.streaming
              : Boolean(row.streaming),
            summary: reasoningSummaryOf(text),
            text
          }
        });
        return;
      }
      const entry = toolEntryByItemId.value.get(String(row.itemId));
      if (!entry || claimed.has(entry.key)) return;
      claimed.add(entry.key);
      rows.push({ kind: 'tool', key: entry.key, entry });
    });
    // 认领不到批次的条目（旧行、投影短暂错位）落到最后一批，条目不会凭空消失。
    if (index === lastActivityIndex) {
      visibleToolEntries.value.forEach((entry) => {
        if (claimed.has(entry.key)) return;
        claimed.add(entry.key);
        rows.push({ kind: 'tool', key: entry.key, entry });
      });
    }
    const toolCount = rows.reduce((count, row) => count + (row.kind === 'tool' ? 1 : 0), 0);
    const latestRow = rows[rows.length - 1];
    return {
      kind: 'activity',
      id: block.id,
      open: activityOpenState.value[block.id] ?? (index === lastActivityIndex && groupOpen.value),
      rows,
      toolCount,
      latestSummary: latestRow
        ? (latestRow.kind === 'tool'
            ? latestRow.entry.summary || latestRow.entry.toolLabel
            : latestRow.entry.summary)
        : '',
      isLast: index === blocks.length - 1
    };
  });
});

const patchViews = computed<Record<string, ToolWorkflowPatchView | null>>(() => {
  const views: Record<string, ToolWorkflowPatchView | null> = {};
  allToolEntries.value.forEach((entry) => {
    if (entry.kind !== 'tool') return;
    views[entry.key] = buildTimelinePatchView(entry.run, t);
  });
  return views;
});

const patchViewFor = (entry: TimelineToolEntry): ToolWorkflowPatchView | null =>
  patchViews.value[entry.key] || null;

// ------------------------------------------------------------ 展开状态

const hasActivityGroup = computed(() =>
  props.blocks.some((block) => block.kind === 'activity')
);
/** 整轮是否默认展开批次栏：主时间线按 MAX_OPEN_TURNS 下发。 */
const groupOpen = ref(props.defaultOpen);
/** 每个批次各自的开合覆盖；未点过的批次跟随整轮默认值。 */
const activityOpenState = ref<Record<string, boolean>>({});
const reasoningOpenKeys = ref<string[]>([]);
const entryOpenKeys = ref<string[]>([]);
const ENTRY_OPEN_LIMIT = MAX_OPEN_ENTRIES_PER_TURN;

watch(
  hasActivityGroup,
  (visible) => emit('activity', Boolean(visible)),
  { immediate: true }
);

const activityTitle = (block: RenderActivityBlock): string =>
  block.toolCount > 0
    ? t('chat.timeline.toolGroupOpen', { count: block.toolCount })
    : t('chat.timeline.processed');

const toggleActivity = (id: string): void => {
  const block = renderBlocks.value.find((item) => item.id === id);
  if (!block || block.kind !== 'activity') return;
  activityOpenState.value = { ...activityOpenState.value, [id]: !block.open };
};

const toggleReasoning = (id: string): void => {
  const next = reasoningOpenKeys.value.filter((key) => key !== id);
  if (next.length === reasoningOpenKeys.value.length) next.push(id);
  reasoningOpenKeys.value = next.slice(-ENTRY_OPEN_LIMIT);
};

const toggleEntry = (key: string): void => {
  const next = new Set(entryOpenKeys.value);
  if (next.has(key)) {
    next.delete(key);
  } else {
    next.add(key);
    while (next.size > ENTRY_OPEN_LIMIT) {
      const firstKey = next.values().next().value as string | undefined;
      if (!firstKey || firstKey === key) break;
      next.delete(firstKey);
    }
  }
  entryOpenKeys.value = Array.from(next);
};

watch(
  () => props.identityKey,
  (key, previousKey) => {
    if (!key || key === previousKey) return;
    // 行/运行身份变化时复位局部展开状态。
    reasoningOpenKeys.value = [];
    entryOpenKeys.value = [];
    activityOpenState.value = {};
    entryRenderLimit.value = TIMELINE_ENTRY_RENDER_LIMIT;
  }
);

// 默认开合由调用方下发；用户点过的批次不再跟随。
watch(
  () => props.defaultOpen,
  (open) => {
    groupOpen.value = Boolean(open);
    activityOpenState.value = {};
  }
);
</script>

<style scoped>
/* §7.3 A BodyBlock：全宽、无气泡；左右内边距 24px、上下 6px；
   hover 底 `hover` 45% 透明；正文 14px `text`。 */
.timeline-body-block {
  position: relative;
  width: 100%;
  min-width: 0;
  padding: 6px 24px;
  color: var(--mz-text, #1f2329);
  font-size: calc(14px * var(--messenger-font-scale, 1));
  line-height: 1.7;
  transition: background-color 0.12s ease;
}

.timeline-body-block:hover {
  background: rgba(246, 245, 243, 0.45);
}

/* ------------------------------------------------------- 工具分组折叠条 */

.timeline-group {
  width: 100%;
  min-width: 0;
}

/* 折叠态是桌面端的「已处理」分隔条（30px、12px muted）；展开态是工具分组条
   （32px、13px text-secondary）。展开箭头压在时间线竖线上（与条目行的节点圆点同
   一列，桌面端 GroupBar 的箭头在内容列），连接线因此只画箭头以下那一段，标题落在
   条目行状态图标同一列（x=52）。 */
.timeline-group-head {
  display: flex;
  align-items: center;
  gap: 8px;
  box-sizing: border-box;
  width: 100%;
  height: 30px;
  padding: 0 24px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}

.timeline-group.is-open .timeline-group-head {
  height: 32px;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 13px;
}

.timeline-group-head:hover {
  background: var(--mz-timeline-hover, #f6f5f3);
}

.timeline-group-gutter {
  position: relative;
  flex: 0 0 auto;
  align-self: stretch;
  width: 20px;
}

.timeline-group-gutter-line {
  position: absolute;
  top: calc(50% + 7px);
  bottom: 0;
  left: 9.5px;
  width: 1px;
  background: var(--mz-timeline-line, #e2dfda);
}

/* 12px 箭头居中于 20px 沟槽的竖线（竖线中心 10px）。 */
.timeline-group-arrow {
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

.timeline-group-arrow.is-open {
  transform: rotate(90deg);
}

.timeline-group-title {
  flex: 0 0 auto;
}

.timeline-group-latest {
  flex: 1 1 auto;
  min-width: 0;
  color: var(--mz-text-muted, #8a8f99);
  font-weight: 400;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 条目行距 0：每条目自带 20px 沟槽，连接线在相邻行之间连续。 */
.timeline-group-body {
  display: flex;
  flex-direction: column;
  gap: 0;
  min-width: 0;
}

.timeline-group-more {
  align-self: flex-start;
  margin: 4px 0 4px 44px;
  padding: 3px 8px;
  border: 1px dashed var(--mz-border-strong, #d8d5d0);
  border-radius: 8px;
  background: transparent;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 11px;
  cursor: pointer;
}

.timeline-group-note {
  margin: 4px 0 4px 44px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
}
</style>
