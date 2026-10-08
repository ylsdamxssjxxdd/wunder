<template>
  <div class="tl-entry" :class="[`is-${entry.status}`, { 'is-open': open }]">
    <component
      :is="interactive ? 'button' : 'div'"
      class="tl-entry-head"
      :type="interactive ? 'button' : undefined"
      :aria-expanded="interactive ? open : undefined"
      @click="handleHeadClick"
    >
      <span class="tl-gutter" aria-hidden="true">
        <span class="tl-gutter-line"></span>
        <span class="tl-gutter-node" :class="`is-${entry.status}`"></span>
      </span>
      <span class="tl-entry-status" :class="`is-${entry.status}`" aria-hidden="true">
        <i v-if="entry.status === 'loading'" class="fa-solid fa-circle-notch fa-spin"></i>
        <i v-else-if="entry.status === 'failed'" class="fa-solid fa-xmark"></i>
        <i v-else-if="entry.status === 'cancelled'" class="fa-solid fa-ban"></i>
        <i v-else class="fa-solid fa-check"></i>
      </span>
      <span v-if="entry.toolLabel" class="tl-entry-tool">
        <i :class="['fa-solid', entry.toolIconClass]" aria-hidden="true"></i>
        <span class="tl-entry-tool-label">{{ entry.toolLabel }}</span>
      </span>
      <span v-if="entry.targets.length" class="tl-entry-chips">
        <button
          v-for="target in entry.targets"
          :key="target.key"
          class="tl-entry-chip"
          type="button"
          :title="target.path"
          @click.stop="revealTarget(target.path)"
        >
          {{ target.label }}
        </button>
      </span>
      <span v-if="entry.summary" class="tl-entry-summary" :title="entry.summary">{{ entry.summary }}</span>
      <i
        v-if="interactive"
        :class="['fa-solid', open ? 'fa-chevron-down' : 'fa-chevron-right', 'tl-entry-toggle']"
        aria-hidden="true"
      ></i>
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

      <div v-else-if="outputText" class="tl-entry-output">
        <div class="tl-entry-output-head">{{ t('chat.timeline.entryOutputTitle') }}</div>
        <pre class="tl-entry-output-body">{{ outputText }}</pre>
      </div>

      <div v-else-if="!entry.errorText" class="tl-entry-empty">
        {{ t('chat.timeline.entryNoDetail') }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';
import { requestWorkspaceReveal } from '@/views/messenger/workspace/workspaceChatReference';
import type { TimelineToolEntry } from './toolTimelineModel';
import type { ToolWorkflowPatchView } from './toolWorkflowTypes';
import MessageToolWorkflowPatchSection from './MessageToolWorkflowPatchSection.vue';
import MessageToolWorkflowCompactionSection from './MessageToolWorkflowCompactionSection.vue';

const props = withDefaults(defineProps<{
  entry: TimelineToolEntry;
  open?: boolean;
  /** 渲染补丁卡片用的视图（由调用方按需构建，未提供时退回正文/错误展示）。 */
  patchView?: ToolWorkflowPatchView | null;
}>(), {
  open: false,
  patchView: null
});

const emit = defineEmits<{ (event: 'toggle'): void }>();

const { t } = useI18n();

const interactive = computed(() => Boolean(props.entry.expandable));

/**
 * 工具输出在 DOM 里必须有界（方案 §7.3 C「长输出限高折叠」+ 性能要求）。
 * 服务端可能返回几十万字的输出，整段塞进 `<pre>` 会同时撑大内存与 `innerText`
 * 成本；这里保留前 N 字并显式标注省略量（与旧工作流条目的有界预览同一口径）。
 */
const OUTPUT_TEXT_LIMIT = 4000;
const outputText = computed(() => {
  const text = String(props.entry.detailText || '');
  if (text.length <= OUTPUT_TEXT_LIMIT) return text;
  return `${text.slice(0, OUTPUT_TEXT_LIMIT)}\n… (${text.length - OUTPUT_TEXT_LIMIT} chars omitted)`;
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
 * 摘要 12px muted 单行省略、右侧展开箭头 12px。
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
.tl-gutter-node {
  position: absolute;
  top: 9px;
  left: 7px;
  width: 6px;
  height: 6px;
  border-radius: 3px;
  background: rgba(62, 156, 126, 0.65);
}

.tl-gutter-node.is-loading {
  background: var(--mz-thought, #3e9c7e);
}

.tl-gutter-node.is-failed,
.tl-gutter-node.is-cancelled {
  background: var(--mz-danger, #d04a43);
}

/* 状态点 16px：完成 = 实心 + 白勾，运行 = 环形 + 旋转图标，失败/取消 = 红底白叉。 */
.tl-entry-status {
  flex: 0 0 auto;
  width: 16px;
  height: 16px;
  box-sizing: border-box;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  font-size: 9px;
  line-height: 1;
  color: #ffffff;
  background: var(--mz-thought, #3e9c7e);
}

.tl-entry-status.is-loading {
  background: transparent;
  border: 2px solid rgba(62, 156, 126, 0.26);
  color: var(--mz-thought, #3e9c7e);
}

.tl-entry-status.is-failed,
.tl-entry-status.is-cancelled {
  background: var(--mz-danger, #d04a43);
}

.tl-entry-tool {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 13px;
}

.tl-entry-tool i {
  font-size: 13px;
  color: var(--mz-text-muted, #8a8f99);
}

.tl-entry-summary {
  flex: 1 1 auto;
  min-width: 0;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  line-height: 1.5;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
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

.tl-entry-chip:hover {
  background: var(--mz-selected, #f1efec);
  color: var(--mz-text-secondary, #3d3d3d);
}

.tl-entry-toggle {
  flex: 0 0 auto;
  width: 12px;
  font-size: 10px;
  color: var(--mz-text-muted, #8a8f99);
}

/* 展开内容左内边距保持 24px（沟槽 20px + 24px = 44px），不被分组沟槽挤压。 */
.tl-entry-body {
  margin: 4px 24px 8px 44px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
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
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  font-weight: 600;
}

.tl-entry-output-body {
  margin: 4px 0 0;
  white-space: pre-wrap;
  word-break: break-word;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 12px;
  line-height: 1.55;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.tl-entry-empty {
  padding: 6px 10px;
  border: 1px dashed var(--mz-border, #e8e6e3);
  border-radius: 8px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
}
</style>
