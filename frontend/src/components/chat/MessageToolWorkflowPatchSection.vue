<template>
  <div class="patch-diff-card">
    <section
      v-for="file in renderFiles"
      :key="file.key"
      :class="['patch-diff-file', file.tone ? `is-${file.tone}` : '']"
    >
      <header class="patch-diff-head">
        <span class="patch-diff-file-icon" aria-hidden="true">
          <i class="fa-regular fa-file-code"></i>
        </span>
        <button
          v-if="file.revealPath"
          class="patch-diff-path"
          type="button"
          :title="file.revealPath"
          @click="revealPath(file.revealPath)"
        >
          {{ file.pathLabel }}
        </button>
        <span v-else class="patch-diff-path is-static" :title="file.pathLabel">{{ file.pathLabel }}</span>
        <span class="patch-diff-stats">
          <span class="patch-diff-added">+{{ file.addedLines }}</span>
          <span class="patch-diff-deleted">−{{ file.deletedLines }}</span>
        </span>
        <span v-if="pending" class="patch-diff-pending">{{ t('chat.timeline.patch.pending') }}</span>
      </header>

      <div v-if="file.meta" class="patch-diff-meta">{{ file.meta }}</div>

      <div v-if="file.rows.length" class="patch-diff-body">
        <div class="patch-diff-table-head">{{ t('chat.timeline.patch.sectionTitle') }}</div>
        <!-- 一次写入可能带 200 行 diff：行区给定高视口自己滚，而不是把整条
             时间线拉成几屏高。原生滚动条被全局隐藏，这里自绘一条细滚条，
             让「还有内容」看得见。 -->
        <div class="patch-diff-viewport">
          <div
            class="patch-diff-lines"
            :ref="(el) => bindLines(file.key, el)"
            @scroll="syncThumb(file.key)"
          >
            <div
              v-for="row in file.rows"
              :key="row.key"
              :class="['patch-diff-row', `is-${row.kind}`]"
            >
              <span class="patch-diff-line-no">{{ row.lineNo }}</span>
              <span class="patch-diff-sign" aria-hidden="true">{{ row.sign }}</span>
              <span class="patch-diff-code">{{ row.code }}</span>
            </div>
          </div>
          <div v-if="thumbOf(file.key).visible" class="patch-diff-scroll" aria-hidden="true">
            <div
              class="patch-diff-scroll-thumb"
              :style="thumbStyle(file.key)"
              @pointerdown="startThumbDrag($event, file.key)"
            ></div>
          </div>
        </div>
        <button
          v-if="file.hiddenLines > 0"
          class="patch-diff-more"
          type="button"
          @click="expandFile(file.key, file.hiddenLines)"
        >
          {{ t('chat.timeline.patch.showMoreLines', { count: file.hiddenLines }) }}
        </button>
      </div>

      <div v-else class="patch-diff-empty">{{ t('chat.timeline.patch.empty') }}</div>
    </section>

    <div v-if="omittedFiles > 0" class="patch-diff-note" role="note">
      {{ t('chat.timeline.patch.omittedFiles', { count: omittedFiles }) }}
    </div>
    <div v-if="pending && !hasAnyRow" class="patch-diff-note" role="note">
      {{ t('chat.timeline.patch.pendingHint') }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onUpdated, ref } from 'vue';

import { useI18n } from '@/i18n';
import { requestWorkspaceReveal } from '@/views/messenger/workspace/workspaceChatReference';
import type { ToolWorkflowPatchFileView, ToolWorkflowPatchLine, ToolWorkflowPatchView } from './toolWorkflowTypes';

/**
 * 补丁 diff 卡片（方案 §7.4）。
 *
 * 单卡片最多渲染 PATCH_CARD_LINE_LIMIT 行；超出部分折叠为「显示更多」，
 * 展开上限为 PATCH_CARD_LINE_EXPANDED_LIMIT，避免一个超大补丁拖垮渲染。
 */
const PATCH_CARD_LINE_LIMIT = 200;
const PATCH_CARD_LINE_EXPANDED_LIMIT = 1200;
const PATCH_CARD_FILE_LIMIT = 8;

type PatchRow = {
  key: string;
  kind: 'add' | 'delete' | 'context' | 'meta' | 'note';
  lineNo: number | null;
  sign: string;
  code: string;
};

type RenderFile = {
  key: string;
  pathLabel: string;
  revealPath: string;
  meta: string;
  tone?: ToolWorkflowPatchFileView['tone'];
  rows: PatchRow[];
  addedLines: number;
  deletedLines: number;
  hiddenLines: number;
};

const props = withDefaults(defineProps<{
  view: ToolWorkflowPatchView;
  /** 工具结果返回前的「待应用」预览标记。 */
  pending?: boolean;
}>(), {
  pending: false
});

const { t } = useI18n();

// 用户「显示更多」的行数预算，按文件 key 记录；有界重建，不保留历史。
const lineBudget = ref<Record<string, number>>({});

const HUNK_HEADER_PATTERN = /^@@\s*-(\d+)(?:,(\d+))?\s+\+(\d+)(?:,(\d+))?\s*@@/;

const stripMetaPrefix = (text: string): string => String(text || '').replace(/^@@\s*/, '@@ ');

const parseHunkStart = (text: string): { oldStart: number; newStart: number } | null => {
  const match = HUNK_HEADER_PATTERN.exec(String(text || '').trim());
  if (!match) return null;
  const oldStart = Number.parseInt(match[1], 10);
  const newStart = Number.parseInt(match[3], 10);
  if (!Number.isFinite(oldStart) || !Number.isFinite(newStart)) return null;
  return { oldStart, newStart };
};

const stripPathNoise = (value: string): string =>
  String(value || '')
    .trim()
    .replace(/^a\//, '')
    .replace(/^b\//, '')
    .replace(/\s+->\s+.*$/, '');

const normalizeRelativePath = (value: string): string => {
  const normalized = stripPathNoise(value).replace(/\\/g, '/').replace(/\/{2,}/g, '/');
  if (!normalized || normalized.startsWith('/') || /^[a-zA-Z]:\//.test(normalized) || normalized.includes('..')) {
    return '';
  }
  return normalized;
};

const buildFileRows = (file: ToolWorkflowPatchFileView, budget: number): {
  rows: PatchRow[];
  hiddenLines: number;
  addedLines: number;
  deletedLines: number;
} => {
  const rows: PatchRow[] = [];
  let oldLine: number | null = null;
  let newLine: number | null = null;
  let addedLines = 0;
  let deletedLines = 0;
  let hiddenLines = 0;
  const sourceLines = Array.isArray(file.lines) ? file.lines : [];

  sourceLines.forEach((line: ToolWorkflowPatchLine, index) => {
    const rawText = String(line?.text ?? '');
    if (line.kind === 'add') addedLines += 1;
    if (line.kind === 'delete') deletedLines += 1;
    if (rows.length >= budget) {
      hiddenLines += 1;
      return;
    }
    if (line.kind === 'meta' || line.kind === 'header') {
      const hunk = parseHunkStart(rawText);
      if (hunk) {
        oldLine = hunk.oldStart;
        newLine = hunk.newStart;
      }
      rows.push({
        key: `meta-${index}`,
        kind: 'meta',
        lineNo: null,
        sign: '',
        code: stripMetaPrefix(rawText)
      });
      return;
    }
    if (line.kind === 'note' || line.kind === 'error') {
      rows.push({
        key: `note-${index}`,
        kind: 'note',
        lineNo: null,
        sign: '',
        code: rawText
      });
      return;
    }
    const explicitOld = typeof line.oldLine === 'number' ? line.oldLine : null;
    const explicitNew = typeof line.newLine === 'number' ? line.newLine : null;
    if (line.kind === 'delete') {
      rows.push({
        key: `del-${index}`,
        kind: 'delete',
        lineNo: explicitOld ?? oldLine,
        sign: '−',
        code: rawText.replace(/^-/, '')
      });
      if (oldLine !== null) oldLine += 1;
      return;
    }
    if (line.kind === 'add') {
      rows.push({
        key: `add-${index}`,
        kind: 'add',
        lineNo: explicitNew ?? newLine,
        sign: '+',
        code: rawText.replace(/^\+/, '')
      });
      if (newLine !== null) newLine += 1;
      return;
    }
    const isContext = line.kind === 'context' || line.kind === 'move' || line.kind === 'update';
    rows.push({
      key: `ctx-${index}`,
      kind: isContext ? 'context' : 'note',
      lineNo: isContext ? (explicitNew ?? newLine) : null,
      sign: line.kind === 'move' ? '>' : line.kind === 'update' ? '~' : '',
      code: rawText.replace(/^[ >~]/, '')
    });
    if (isContext) {
      if (oldLine !== null) oldLine += 1;
      if (newLine !== null) newLine += 1;
    }
  });

  return { rows, hiddenLines, addedLines, deletedLines };
};

const resolvePathLabel = (file: ToolWorkflowPatchFileView): string => {
  const title = String(file.title || '').trim();
  if (title) return title;
  return String(file.key || '').trim();
};

const clampBudget = (key: string): number => {
  const requested = Number(lineBudget.value[key] || 0);
  const budget = requested > 0 ? requested : PATCH_CARD_LINE_LIMIT;
  return Math.min(Math.max(budget, PATCH_CARD_LINE_LIMIT), PATCH_CARD_LINE_EXPANDED_LIMIT);
};

const renderFiles = computed<RenderFile[]>(() => {
  const files = Array.isArray(props.view?.files) ? props.view.files : [];
  return files.slice(0, PATCH_CARD_FILE_LIMIT).map((file) => {
    const budget = clampBudget(String(file.key || ''));
    const built = buildFileRows(file, budget);
    const revealPath = normalizeRelativePath(resolvePathLabel(file));
    return {
      key: String(file.key || resolvePathLabel(file)),
      pathLabel: resolvePathLabel(file),
      revealPath,
      meta: String(file.meta || ''),
      tone: file.tone,
      rows: built.rows,
      addedLines: built.addedLines,
      deletedLines: built.deletedLines,
      hiddenLines: built.hiddenLines
    };
  });
});

const omittedFiles = computed(() => {
  const declared = Number(props.view?.omittedFiles || 0);
  const files = Array.isArray(props.view?.files) ? props.view.files.length : 0;
  return Math.max(declared, Math.max(files - PATCH_CARD_FILE_LIMIT, 0));
});

const hasAnyRow = computed(() => renderFiles.value.some((file) => file.rows.length > 0));

const expandFile = (key: string, hiddenLines: number): void => {
  const current = clampBudget(key);
  lineBudget.value = {
    ...lineBudget.value,
    [key]: Math.min(current + Math.max(hiddenLines, PATCH_CARD_LINE_LIMIT), PATCH_CARD_LINE_EXPANDED_LIMIT)
  };
};

const revealPath = (path: string): void => {
  requestWorkspaceReveal(path);
};

/**
 * 自绘细滚条：云端前端全局隐藏了原生滚动条（styles/base.css），
 * diff 行区限高后必须自己给出「还有内容」的提示，否则看起来像被裁掉。
 * 只在真的溢出时出现，位置随滚动更新，可按住拖动。
 */
type ThumbState = { visible: boolean; height: number; offset: number };

const EMPTY_THUMB: ThumbState = { visible: false, height: 0, offset: 0 };

const lineShells = new Map<string, HTMLElement>();
const thumbs = ref<Record<string, ThumbState>>({});

const thumbOf = (key: string): ThumbState => thumbs.value[key] || EMPTY_THUMB;

const thumbStyle = (key: string): Record<string, string> => {
  const thumb = thumbOf(key);
  if (!thumb.visible) return {};
  return {
    height: thumb.height + 'px',
    transform: 'translateY(' + thumb.offset + 'px)'
  };
};

const syncThumb = (key: string): void => {
  const shell = lineShells.get(key);
  if (!shell) return;
  const viewport = shell.clientHeight;
  const total = shell.scrollHeight;
  let next = EMPTY_THUMB;
  if (viewport > 0 && total > viewport + 1) {
    const height = Math.max(20, Math.round((viewport * viewport) / total));
    const maxTop = Math.max(0, viewport - height);
    const scrollable = Math.max(1, total - viewport);
    const progress = Math.min(1, Math.max(0, shell.scrollTop / scrollable));
    next = { visible: true, height, offset: Math.round(progress * maxTop) };
  }
  const current = thumbOf(key);
  if (
    current.visible === next.visible &&
    current.height === next.height &&
    current.offset === next.offset
  ) {
    return;
  }
  thumbs.value = { ...thumbs.value, [key]: next };
};

const syncAllThumbs = (): void => {
  lineShells.forEach((_shell, key) => syncThumb(key));
};

/** 行数随流式输出增长时重算滚条；等值比较保证不会自激。 */
onUpdated(syncAllThumbs);

const bindLines = (key: string, el: unknown): void => {
  if (el instanceof HTMLElement) {
    lineShells.set(key, el);
    syncThumb(key);
    return;
  }
  lineShells.delete(key);
  if (thumbs.value[key]) {
    const rest = { ...thumbs.value };
    delete rest[key];
    thumbs.value = rest;
  }
};

const startThumbDrag = (event: PointerEvent, key: string): void => {
  const shell = lineShells.get(key);
  if (!shell) return;
  event.preventDefault();
  const thumbHeight = thumbOf(key).height || 20;
  const span = Math.max(1, shell.clientHeight - thumbHeight);
  const top = shell.getBoundingClientRect().top;
  const move = (pointer: PointerEvent) => {
    const ratio = Math.min(1, Math.max(0, (pointer.clientY - top - thumbHeight / 2) / span));
    shell.scrollTop = ratio * (shell.scrollHeight - shell.clientHeight);
    syncThumb(key);
  };
  const stop = () => {
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', stop);
    window.removeEventListener('pointercancel', stop);
  };
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', stop);
  window.addEventListener('pointercancel', stop);
  move(event);
};
</script>

<style scoped>
.patch-diff-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
}

.patch-diff-file {
  border: 1px solid var(--mz-border);
  border-radius: 10px;
  background: var(--mz-surface);
  overflow: hidden;
  contain: paint;
}

.patch-diff-file.is-danger {
  border-color: rgba(220, 38, 38, 0.28);
}

.patch-diff-file.is-warning {
  border-color: rgba(217, 119, 6, 0.28);
}

.patch-diff-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--mz-border);
  background: var(--mz-surface-soft);
  min-width: 0;
}

.patch-diff-file-icon {
  flex: 0 0 auto;
  color: var(--mz-text-muted);
  font-size: 12px;
}

.patch-diff-path {
  flex: 1 1 auto;
  min-width: 0;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--mz-text);
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
  font-size: 12.5px;
  font-weight: 600;
  text-align: left;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
}

.patch-diff-path:hover {
  color: var(--ui-accent-ink);
  text-decoration: underline;
}

.patch-diff-path.is-static {
  cursor: default;
}

.patch-diff-stats {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: baseline;
  gap: 8px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 12px;
  font-weight: 600;
}

.patch-diff-added {
  color: var(--mz-success);
}

.patch-diff-deleted {
  color: var(--mz-danger);
}

.patch-diff-pending {
  flex: 0 0 auto;
  padding: 1px 7px;
  border-radius: 999px;
  background: var(--mz-warning-bg);
  color: var(--mz-warning);
  font-size: 11px;
  font-weight: 600;
}

.patch-diff-meta {
  padding: 4px 10px 0;
  color: var(--mz-text-muted);
  font-size: 11.5px;
}

.patch-diff-body {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

/* diff 行区的固定视口：约 16 行，超出部分内部滚动。卡片头部与
   「显示更多」留在视口外，长 diff 不会把整条时间线撑成几屏。 */
.patch-diff-viewport {
  position: relative;
  min-width: 0;
}

.patch-diff-lines {
  max-height: 320px;
  overflow: auto;
  overscroll-behavior: contain;
}

/* 自绘细滚条（原生滚动条被全局隐藏，见 styles/base.css）。 */
.patch-diff-scroll {
  position: absolute;
  top: 3px;
  right: 3px;
  bottom: 3px;
  width: 6px;
  pointer-events: none;
}

.patch-diff-scroll-thumb {
  width: 6px;
  border-radius: 999px;
  background: var(--mz-border-strong);
  pointer-events: auto;
  cursor: pointer;
  transition: background 120ms ease;
}

.patch-diff-scroll-thumb:hover {
  background: var(--mz-text-muted);
}

.patch-diff-table-head {
  padding: 5px 10px;
  background: var(--mz-surface-soft);
  color: var(--mz-text-muted);
  font-size: 11.5px;
  font-weight: 600;
  border-bottom: 1px solid var(--mz-border);
}

.patch-diff-row {
  display: grid;
  grid-template-columns: 48px 14px minmax(0, 1fr);
  align-items: start;
  column-gap: 4px;
  padding: 1px 10px 1px 0;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
  font-size: 12.5px;
  line-height: 1.55;
}

.patch-diff-line-no {
  color: var(--mz-text-muted);
  text-align: right;
  user-select: none;
  font-variant-numeric: tabular-nums;
}

.patch-diff-sign {
  color: var(--mz-text-muted);
  text-align: center;
  user-select: none;
}

.patch-diff-code {
  min-width: 0;
  white-space: pre-wrap;
  word-break: break-word;
  color: var(--mz-text);
}

.patch-diff-row.is-add {
  background: var(--mz-success-bg);
}

.patch-diff-row.is-add .patch-diff-sign,
.patch-diff-row.is-add .patch-diff-code {
  color: var(--mz-success);
}

.patch-diff-row.is-delete {
  background: var(--mz-danger-bg);
}

.patch-diff-row.is-delete .patch-diff-sign,
.patch-diff-row.is-delete .patch-diff-code {
  color: var(--mz-danger);
}

.patch-diff-row.is-meta,
.patch-diff-row.is-note {
  grid-template-columns: minmax(0, 1fr);
  padding: 3px 10px;
  background: var(--mz-surface-soft);
  color: var(--mz-text-muted);
  font-size: 11.5px;
}

.patch-diff-more {
  align-self: stretch;
  margin: 6px 10px 8px;
  padding: 4px 8px;
  border: 1px dashed var(--mz-border);
  border-radius: 8px;
  background: transparent;
  color: var(--mz-text-muted);
  font-size: 11.5px;
  cursor: pointer;
}

.patch-diff-more:hover {
  border-color: var(--mz-border-strong);
  color: var(--mz-text-secondary);
}

.patch-diff-empty,
.patch-diff-note {
  padding: 6px 10px;
  color: var(--mz-text-muted);
  font-size: 11.5px;
}

.patch-diff-note {
  border: 1px dashed var(--mz-border);
  border-radius: 8px;
}
</style>
