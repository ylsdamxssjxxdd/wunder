<template>
  <div class="tl-entry" :class="[`is-${displayStatus}`, { 'is-open': open, 'is-command': isCommand }]">
    <component
      :is="interactive ? 'button' : 'div'"
      class="tl-entry-head"
      :type="interactive ? 'button' : undefined"
      :aria-expanded="interactive ? open : undefined"
      @click="handleHeadClick"
    >
      <span class="tl-gutter" aria-hidden="true">
        <span class="tl-gutter-line"></span>
        <span class="tl-entry-status" :class="`is-${displayStatus}`">
          <i v-if="displayStatus === 'loading'" class="fa-solid fa-circle-notch fa-spin"></i>
          <i v-else-if="displayStatus === 'failed' || displayStatus === 'cancelled'" class="fa-solid fa-xmark"></i>
          <i v-else class="fa-solid fa-check"></i>
        </span>
      </span>
      <span v-if="entry.toolLabel" class="tl-entry-tool">
        <i :class="['fa-solid', entry.toolIconClass]" aria-hidden="true"></i>
        <span class="tl-entry-tool-label">{{ commandToolLabel || rowLabel }}</span>
      </span>
      <span v-if="entry.targets.length" class="tl-entry-chips">
        <!-- 徽章只显示文件名。绝对路径（云端沙箱的物理路径）无法在左栏定位，
             仍要露出来源文件名，此时降级为不可点击的静态徽章。 -->
        <template v-for="target in entry.targets" :key="target.key">
          <button
            v-if="target.path"
            class="tl-entry-chip"
            type="button"
            :title="target.title || target.path"
            @click.stop="revealTarget(target.path)"
          >
            {{ target.label }}
          </button>
          <span v-else class="tl-entry-chip is-static" :title="target.title || target.label">
            {{ target.label }}
          </span>
        </template>
      </span>
      <!-- 改动规模：与补丁卡片头部同口径，只显示真正存在的一侧，避免「+0 −0」。 -->
      <span
        v-if="entry.diffStats"
        class="tl-entry-diff"
        :title="t('chat.timeline.entryDiffStats', { added: entry.diffStats.added, deleted: entry.diffStats.deleted })"
      >
        <span v-if="entry.diffStats.added > 0" class="tl-entry-diff-add">+{{ entry.diffStats.added }}</span>
        <span v-if="entry.diffStats.deleted > 0" class="tl-entry-diff-del">−{{ entry.diffStats.deleted }}</span>
      </span>
      <!-- 读取量：与编辑行的 +/- 同一位置，窗口读取带上文件总行数。 -->
      <span v-if="entry.readLines" class="tl-entry-read-lines">{{ readLinesLabel }}</span>
      <span
        v-if="commandSummary || entry.summary"
        class="tl-entry-summary"
        :class="{ 'is-mono': entry.summaryMono }"
        :title="commandSummary || entry.summary"
      >{{ commandSummary || entry.summary }}</span>
      <!-- 与桌面端折叠行一致：右侧依次是模型轮次、该次请求的上下文占用与工具耗时。
           各列定宽且左对齐，跨条目的数值从同一位置起排；轮次为空也占列。 -->
      <span v-if="roundLabel || tokenMetric || durationMetric" class="tl-entry-metrics">
        <span>{{ roundLabel }}</span>
        <span>{{ tokenMetric }}</span>
        <span>{{ durationMetric }}</span>
      </span>
    </component>

    <div v-if="interactive && open" class="tl-entry-body">
      <div v-if="entry.errorText" class="tl-entry-error" role="note">
        <div class="tl-entry-error-title">{{ t('chat.timeline.entryFailedTitle') }}</div>
        <pre class="tl-entry-error-text">{{ entry.errorText }}</pre>
      </div>

      <MessageToolWorkflowPatchSection
        v-if="patchView"
        :view="patchView"
        :pending="entry.status === 'loading'"
      />

      <MessageToolWorkflowCompactionSection
        v-else-if="entry.compaction"
        :view="entry.compaction.view"
      />

      <MessageToolWorkflowStructuredSection
        v-else-if="structuredView"
        :view="structuredView"
      />

      <div v-else-if="outputText || isCommand" class="tl-entry-output" :class="{ 'is-command-output': isCommand }">
        <div class="tl-entry-output-head">
          <span>{{ t('chat.timeline.entryOutputTitle') }}</span>
          <span v-if="commandSession?.cwd" class="tl-entry-output-cwd">{{ commandSession.cwd }}</span>
          <span
            v-if="displayExitCode !== null"
            class="tl-entry-output-exit"
            :class="{ 'is-nonzero': displayExitCode !== 0 }"
          >
            {{ t('chat.timeline.entryOutputExitCode', { code: displayExitCode }) }}
          </span>
        </div>
        <pre v-if="interactionInput" class="tl-entry-input">stdin › {{ interactionInput }}</pre>
        <pre v-if="outputText" class="tl-entry-output-body">{{ outputText }}</pre>
        <pre v-else class="tl-entry-output-body is-empty">{{ t('chat.timeline.commandNoOutput') }}</pre>
      </div>

      <div v-else-if="!entry.errorText" class="tl-entry-output">
        <div class="tl-entry-output-head">{{ fallbackStatus }}</div>
        <pre v-if="callPreview" class="tl-entry-output-body">{{ callPreview }}</pre>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';
import { requestWorkspaceReveal } from '@/views/messenger/workspace/workspaceChatReference';
import type { TimelineToolEntry } from './toolTimelineModel';
import type { ToolWorkflowPatchView, ToolWorkflowStructuredView } from './toolWorkflowTypes';
import MessageToolWorkflowPatchSection from './MessageToolWorkflowPatchSection.vue';
import MessageToolWorkflowCompactionSection from './MessageToolWorkflowCompactionSection.vue';
import MessageToolWorkflowStructuredSection from './MessageToolWorkflowStructuredSection.vue';
import { extractWorkflowCallArgs } from './toolWorkflowCallDebug';
import { parseToolWorkflowResultObject } from './toolWorkflowResultPayload';
import { buildToolResultPreview } from './toolWorkflowPreview';
import { useCommandSessionStore } from '@/stores/commandSessions';
import {
  commandSessionOutputPreview,
  resolveCommandDisplayStatus,
  resolveCommandInteractionInput,
  resolveTimelineCommandSessionRef
} from './toolTimelineCommandSession';
import { formatDurationLabel, formatTokenLabel } from './toolWorkflowCollapsedMetadata';
import { toolRowLabel } from './toolTimelineLabel';

const props = withDefaults(defineProps<{
  entry: TimelineToolEntry;
  open?: boolean;
  /** 渲染补丁卡片用的视图（由调用方按需构建，未提供时退回正文/错误展示）。 */
  patchView?: ToolWorkflowPatchView | null;
  /** 结构化结果视图（定时任务/读写/检索等），优先级低于补丁与压缩卡。 */
  structuredView?: ToolWorkflowStructuredView | null;
}>(), {
  open: false,
  patchView: null,
  structuredView: null
});

const emit = defineEmits<{ (event: 'toggle'): void }>();

const { t } = useI18n();
const commandStore = useCommandSessionStore();
const commandSessionId = computed(() => resolveTimelineCommandSessionRef(props.entry.run));
const commandSession = computed(() => commandSessionId.value
  ? commandStore.getById(commandSessionId.value)
  : commandStore.getByToolCallId(String(props.entry.run.callItem?.toolCallId ??
      props.entry.run.callItem?.tool_call_id ?? props.entry.run.key)));
const isCommand = computed(() => props.entry.commandInteraction !== null);
// 命令类工具在行首只写一个动词「执行」：是刚发起、写 stdin 还是轮询新输出，
// 由命令文本与状态点说明，标签不再分叉成「正在执行 / 发送输入 / 等待」。
const commandToolLabel = computed(() =>
  isCommand.value ? t('chat.timeline.commandRan') : ''
);
// 其它工具收成两字（读取/编辑/列出/搜索…），与桌面版、安卓版同一张词表。
// `entry.toolLabel` 的原名仍留给折叠摘要与结构化视图使用。
const rowLabel = computed(() => toolRowLabel(props.entry.toolLabel));
const commandSummary = computed(() =>
  isCommand.value ? String(commandSession.value?.command || props.entry.summary || '') : ''
);
const displayStatus = computed(() => resolveCommandDisplayStatus(
  props.entry.status,
  props.entry.commandSettled,
  commandSession.value
));
const displayExitCode = computed(() => commandSession.value?.exitCode ?? props.entry.exitCode);
const tokenMetric = computed(() => formatTokenLabel(props.entry.metrics.tokens));
const durationMetric = computed(() => formatDurationLabel(props.entry.metrics.durationMs));
/** 模型轮次与桌面端同形：R1、R2…；旧记录没有轮次时留空。 */
const roundLabel = computed(() => {
  const round = props.entry.modelRound ?? null;
  return round !== null && round > 0 ? `R${round}` : '';
});
const readLinesLabel = computed(() => {
  const lines = props.entry.readLines;
  if (!lines) return '';
  return lines.total !== null && lines.total > lines.read
    ? t('chat.timeline.entryReadLinesPartial', { read: lines.read, total: lines.total })
    : t('chat.timeline.entryReadLines', { lines: lines.read });
});

const interactive = computed(() => Boolean(isCommand.value || props.entry.expandable || commandSession.value));
const fallbackStatus = computed(() => t({
  loading: 'chat.toolWorkflow.toolResultPending',
  completed: 'chat.toolWorkflow.resultDone',
  failed: 'chat.toolWorkflow.resultFailed',
  cancelled: 'chat.toolWorkflow.statusCancelled'
}[displayStatus.value]));
// Only build the fallback when the user opens an entry with no result body.
const callPreview = computed(() => {
  if (!props.open || props.entry.detailText || props.structuredView || props.patchView || props.entry.errorText) return '';
  const args = extractWorkflowCallArgs(props.entry.run.callItem, parseToolWorkflowResultObject);
  if (!args) return '';
  const preview = buildToolResultPreview(args, null);
  return preview && preview !== '{}' ? `${t('chat.timeline.entryArgumentsTitle')}\n${preview}` : '';
});

/**
 * 工具输出在 DOM 里必须有界（方案 §7.3 C「长输出限高折叠」+ 性能要求）。
 * 服务端可能返回几十万字的输出，整段塞进 `<pre>` 会同时撑大内存与 `innerText`
 * 成本；这里保留开头与最新尾部并显式标注省略量。
 */
const commandRawOutput = computed(() => String(
  commandSession.value?.outputTail || props.entry.detailText || commandSession.value?.error || ''
));
const interactionInput = computed(() => {
  if (props.entry.commandInteraction !== 'write') return '';
  const input = resolveCommandInteractionInput(props.entry.run)
    .replace(/\r/g, '\\r')
    .replace(/\n/g, '\\n')
    .replace(/\t/g, '\\t');
  return input.length > 160 ? `${input.slice(0, 157)}...` : input;
});
const outputText = computed(() => {
  if (isCommand.value) {
    return props.open ? commandSessionOutputPreview(commandRawOutput.value) : '';
  }
  const command = props.open ? commandSession.value : null;
  if (command) {
    const text = [command.ptyTail, command.stdoutTail, command.stderrTail, command.error].filter(Boolean).join('\n');
    if (text) return commandSessionOutputPreview(text);
  }
  return commandSessionOutputPreview(String(props.entry.detailText || ''));
});

const handleHeadClick = (): void => {
  if (!interactive.value) return;
  emit('toggle');
};

const revealTarget = (path: string): void => {
  requestWorkspaceReveal(path);
};
</script>

<style scoped>
/*
 * §7.3 B/C 工具条目行（对齐桌面端 `timeline.slint` 的 `FoldEntry`）：
 * 行高 26px、间距 8px、图标 13px、工具名 13px `text-secondary`、
 * 目标 chip 18px/圆角 6/底 `hover`/字 12px muted/最宽 180px、
 * 摘要 12px muted 单行省略、行首状态点 12px（不显示右侧展开箭头）。
 * 行首是 20px 沟槽：1px 连接线居中 + 6px 节点圆点（y=9px）。
 */
.tl-entry {
  min-width: 0;
}

.tl-entry-head {
  display: flex;
  align-items: center;
  gap: 8px;
  box-sizing: border-box;
  width: 100%;
  height: 26px;
  padding: 0 24px;
  margin: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: default;
}

button.tl-entry-head {
  cursor: pointer;
}

button.tl-entry-head:hover {
  background: var(--mz-timeline-hover, #f6f5f3);
}

button.tl-entry-head:focus-visible {
  outline: 2px solid rgba(var(--ui-accent-rgb), 0.4);
  outline-offset: -2px;
}

/* 沟槽本身不是点击热区（桌面端 `head-hit` 从 `padding + gutter` 起算）。 */
.tl-gutter {
  position: relative;
  flex: 0 0 auto;
  align-self: stretch;
  width: 20px;
  pointer-events: none;
}

.tl-gutter-line {
  position: absolute;
  top: 0;
  bottom: 0;
  left: 9.5px;
  width: 1px;
  background: var(--mz-timeline-line, #e2dfda);
}

/* 节点圆点：已完成 = `thought` 65% 透明（沿用桌面端 `transparentize(0.35)`），
   运行中 = `thought` 实色，失败/取消 = `danger`。 */
.tl-entry-status {
  position: absolute;
  z-index: 1;
  top: 7px;
  left: 3.5px;
  width: 12px;
  height: 12px;
  box-sizing: border-box;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--mz-timeline-entry-soft, #9ba0a8);
  border-radius: 50%;
  font-size: 7px;
  line-height: 1;
  color: var(--mz-text-muted, #8a8f99);
  background: var(--mz-panel, #fbfaf8);
}

.tl-entry-status.is-loading {
  border: 0;
  font-size: 12px;
  color: var(--mz-text-muted, #8a8f99);
}

.tl-entry-status.is-failed,
.tl-entry-status.is-cancelled {
  border-color: var(--mz-danger, #d04a43);
  color: var(--mz-danger, #d04a43);
}

.tl-entry-tool {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 13px;
  /* 与右侧摘要、chip、行数徽标共用 18px 行高，混排时文字落在同一水平线。 */
  line-height: 18px;
}

.tl-entry-tool i {
  font-size: 13px;
  color: var(--mz-text-muted, #8a8f99);
}

.tl-entry-summary {
  flex: 1 1 auto;
  min-width: 0;
  color: var(--mz-timeline-entry-soft, #9ba0a8);
  font-size: 12px;
  line-height: 18px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 命令形态的摘要用等宽字体：与输出面板同一族，读起来是「一条命令」。 */
.tl-entry-summary.is-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 11.5px;
}

/* 右侧运行指标：与桌面端 ProcessEntryCard 的 R/tokens/duration 同口径。
   固定三列（3ch/6ch/5ch + 8px），空值也占位，跨条目从同一位置起排。 */
.tl-entry-metrics {
  flex: 0 0 auto;
  display: inline-grid;
  grid-template-columns: 3ch 6ch 5ch;
  column-gap: 8px;
  justify-items: start;
  margin-left: auto;
  color: var(--mz-text-muted, #8a8f99);
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 11px;
  line-height: 18px;
  white-space: nowrap;
}

.tl-entry-chips {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
}

.tl-entry-chip {
  box-sizing: border-box;
  display: inline-block;
  max-width: 180px;
  height: 18px;
  padding: 0 7px;
  border: 0;
  border-radius: 6px;
  background: var(--mz-timeline-hover, #f6f5f3);
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  line-height: 18px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
}

/* 不可定位的徽章（绝对路径）只是文件名标签，不再有悬停/点击暗示。 */
.tl-entry-chip.is-static {
  cursor: default;
}

button.tl-entry-chip:hover {
  background: var(--mz-selected, #f1efec);
  color: var(--mz-text-secondary, #3d3d3d);
}

/* 编辑/补丁条目的 +/- 行数：与补丁卡片头部同一配色（绿加红减），等宽对齐。 */
.tl-entry-diff {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: baseline;
  gap: 6px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 11.5px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  line-height: 18px;
}

.tl-entry-diff-add {
  color: #16a34a;
}

.tl-entry-diff-del {
  color: #dc2626;
}

/* 读取条目的返回行数：与 +/- 同位置，弱化为灰色等宽，不抢状态色。 */
.tl-entry-read-lines {
  flex: 0 0 auto;
  color: var(--mz-text-muted, #8a8f99);
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 11.5px;
  font-variant-numeric: tabular-nums;
  line-height: 18px;
  white-space: nowrap;
}

/* 展开内容左内边距保持 24px（沟槽 20px + 24px = 44px），不被分组沟槽挤压。 */
.tl-entry-body {
  margin: 4px 24px 8px 44px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
  /* 展开区复用的结构化/补丁/压缩分区原本长在 MessageToolWorkflow 的深色外壳
     里，边框与底色全靠那层外壳上的 `--workflow-term-*`。时间线没有那层外壳，
     `var()` 解析失败会让整条 `border` 声明失效（看图、搜索等条目展开后像一段
     裸文本）。这里按时间线的浅色令牌补齐基线；外层仍是深色工作流时由它自己的
     变量覆盖，不受影响。 */
  --workflow-term-bg: var(--mz-panel, #fbfaf8);
  --workflow-term-bg-soft: var(--mz-panel, #fbfaf8);
  --workflow-term-bg-hover: var(--mz-timeline-hover, #f6f5f3);
  --workflow-term-border: var(--mz-border, #e8e6e3);
  --workflow-term-border-strong: var(--mz-border-strong, #d8d5d0);
  --workflow-term-text: var(--mz-text-secondary, #3d3d3d);
  --workflow-term-muted: var(--mz-text-muted, #8a8f99);
  --workflow-term-code: var(--mz-text-secondary, #3d3d3d);
  --workflow-term-scroll-track: transparent;
  --workflow-term-scroll-thumb: var(--mz-border-strong, #d8d5d0);
}

.tl-entry-error {
  border: 1px solid rgba(208, 74, 67, 0.22);
  border-radius: 8px;
  background: var(--mz-danger-bg, #fdecec);
  padding: 8px 10px;
}

.tl-entry-error-title {
  color: var(--mz-danger, #d04a43);
  font-size: 12px;
  font-weight: 700;
}

.tl-entry-error-text {
  margin: 4px 0 0;
  max-height: 180px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  color: #7f1d1d;
  font-size: 12px;
  line-height: 1.5;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

/* 工具输出面板：最高 240px、圆角 8、底 `panel`、边框 `border`、内部可滚。 */
.tl-entry-output {
  box-sizing: border-box;
  max-height: 240px;
  overflow: auto;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 8px;
  background: var(--mz-panel, #fbfaf8);
  padding: 8px 10px;
}

.tl-entry-output-head {
  display: flex;
  align-items: baseline;
  gap: 8px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  font-weight: 600;
}

.tl-entry-output-cwd {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 400;
}

.tl-entry-input {
  margin: 7px 0 0;
  color: var(--mz-text-secondary, #3d3d3d);
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 11.5px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.tl-entry-output-body.is-empty {
  opacity: 0.72;
}

/* 命令类工具的退出码：数字用等宽、成功弱化、非 0 走危险色。 */
.tl-entry-output-exit {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-weight: 500;
}

.tl-entry-output-exit.is-nonzero {
  color: var(--mz-danger, #d04a43);
}

.tl-entry-output-body {
  margin: 4px 0 0;
  white-space: pre-wrap;
  word-break: break-word;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  line-height: 1.55;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}
</style>
