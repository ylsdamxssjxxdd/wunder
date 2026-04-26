<template>
  <div class="tool-workflow-patch-card">
    <div v-if="view.metrics.length" class="tool-workflow-patch-metrics">
      <div
        v-for="metric in view.metrics"
        :key="metric.key"
        :class="['tool-workflow-patch-metric', metric.tone ? `is-${metric.tone}` : '']"
      >
        <span class="tool-workflow-patch-metric-label">{{ metric.label }}</span>
        <span class="tool-workflow-patch-metric-value">{{ metric.value }}</span>
      </div>
    </div>

    <div class="tool-workflow-patch-files">
      <section
        v-for="file in view.files"
        :key="file.key"
        :class="['tool-workflow-patch-file', file.tone ? `is-${file.tone}` : '']"
      >
        <header class="tool-workflow-patch-file-head">
          <span class="tool-workflow-patch-file-title">{{ file.title }}</span>
          <span v-if="file.meta" class="tool-workflow-patch-file-meta">{{ file.meta }}</span>
        </header>

        <div v-if="file.lines.length" class="tool-workflow-patch-file-lines">
          <div
            v-for="line in file.lines"
            :key="line.key"
            :class="['tool-workflow-patch-line', `is-${line.kind}`]"
          >
            <div class="tool-workflow-patch-line-gutter">
              <span class="tool-workflow-patch-line-no">{{ resolveDisplayedLineNo(line) }}</span>
            </div>
            <div class="tool-workflow-patch-line-body">
              <span class="tool-workflow-patch-line-sign" aria-hidden="true">{{ resolveLineSign(line.kind) }}</span>
              <span class="tool-workflow-patch-line-text">{{ line.text }}</span>
            </div>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { ToolWorkflowPatchLine, ToolWorkflowPatchView } from './toolWorkflowTypes';

defineProps<{
  view: ToolWorkflowPatchView;
}>();

const resolveLineSign = (kind: ToolWorkflowPatchLine['kind']): string => {
  if (kind === 'add') return '+';
  if (kind === 'delete') return '-';
  if (kind === 'move') return '>';
  if (kind === 'update') return '~';
  if (kind === 'header') return '@@';
  if (kind === 'meta') return '@@';
  if (kind === 'error') return '!';
  return ' ';
};

const resolveDisplayedLineNo = (line: ToolWorkflowPatchLine): string | number => {
  if (line.kind === 'delete') return line.oldLine ?? '';
  if (line.kind === 'add') return line.newLine ?? '';
  return line.newLine ?? line.oldLine ?? '';
};
</script>

<style scoped>
.tool-workflow-patch-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.tool-workflow-patch-metrics {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.tool-workflow-patch-metric {
  display: inline-flex;
  align-items: baseline;
  gap: 6px;
  padding: 6px 8px;
  border-radius: 999px;
  border: 1px solid var(--workflow-term-border);
  background: rgba(15, 23, 42, 0.16);
}

.tool-workflow-patch-metric.is-success {
  border-color: rgba(74, 222, 128, 0.24);
  background: rgba(22, 163, 74, 0.1);
}

.tool-workflow-patch-metric.is-warning {
  border-color: rgba(251, 191, 36, 0.24);
  background: rgba(217, 119, 6, 0.1);
}

.tool-workflow-patch-metric.is-danger {
  border-color: rgba(248, 113, 113, 0.24);
  background: rgba(127, 29, 29, 0.18);
}

.tool-workflow-patch-metric-label,
.tool-workflow-patch-metric-value {
  font-size: 11px;
  line-height: 1.4;
}

.tool-workflow-patch-metric-label {
  color: var(--workflow-term-muted);
  font-weight: 600;
}

.tool-workflow-patch-metric-value {
  color: var(--workflow-term-text);
}

.tool-workflow-patch-files {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-patch-file {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px;
  border-radius: 10px;
  border: 1px solid var(--workflow-term-border);
  background: var(--workflow-term-bg-soft);
}

.tool-workflow-patch-file-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
}

.tool-workflow-patch-file-title {
  min-width: 0;
  color: var(--workflow-term-text);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.45;
  word-break: break-word;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-patch-file-meta {
  flex: 0 0 auto;
  color: var(--workflow-term-muted);
  font-size: 11px;
}

.tool-workflow-patch-file-lines {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.tool-workflow-patch-line {
  --patch-gutter-column-width: 56px;
  --patch-sign-width: 2ch;
  display: grid;
  grid-template-columns: var(--patch-gutter-column-width) minmax(0, 1fr);
  align-items: start;
  column-gap: 0;
  padding: 0;
  border-radius: 6px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
  overflow: hidden;
}

.tool-workflow-patch-line-gutter {
  display: flex;
  align-items: stretch;
  background: rgba(15, 23, 42, 0.18);
  border-right: 1px solid rgba(148, 163, 184, 0.14);
}

.tool-workflow-patch-line-body {
  display: grid;
  grid-template-columns: var(--patch-sign-width) minmax(0, 1fr);
  align-items: start;
  column-gap: 6px;
  padding: 2px 8px 2px 6px;
  border-left: 2px solid transparent;
  min-width: 0;
}

.tool-workflow-patch-line-sign {
  color: var(--workflow-term-muted);
  text-align: left;
  user-select: none;
}

.tool-workflow-patch-line-no {
  display: block;
  width: 100%;
  color: var(--workflow-term-muted);
  font-size: 11px;
  text-align: left;
  user-select: none;
  font-variant-numeric: tabular-nums;
  opacity: 0.94;
  padding: 2px 6px 2px 10px;
}

.tool-workflow-patch-line-text {
  white-space: pre-wrap;
  word-break: break-word;
  text-align: left;
  justify-self: start;
  width: 100%;
  padding-left: 0;
}

.tool-workflow-patch-line.is-meta .tool-workflow-patch-line-no {
  display: none;
}

.tool-workflow-patch-line.is-meta .tool-workflow-patch-line-gutter {
  display: none;
}

.tool-workflow-patch-line.is-note .tool-workflow-patch-line-no {
  display: none;
}

.tool-workflow-patch-line.is-note .tool-workflow-patch-line-gutter {
  display: none;
}

.tool-workflow-patch-line.is-header .tool-workflow-patch-line-no {
  display: none;
}

.tool-workflow-patch-line.is-header .tool-workflow-patch-line-gutter {
  display: none;
}

.tool-workflow-patch-line.is-note .tool-workflow-patch-line-text {
  font-weight: 600;
}

.tool-workflow-patch-line.is-header .tool-workflow-patch-line-text {
  font-size: 11px;
  line-height: 1.45;
}

.tool-workflow-patch-line.is-header .tool-workflow-patch-line-body,
.tool-workflow-patch-line.is-meta .tool-workflow-patch-line-body,
.tool-workflow-patch-line.is-note .tool-workflow-patch-line-body {
  grid-column: 1 / -1;
  padding: 4px 10px;
}

.tool-workflow-patch-line.is-header .tool-workflow-patch-line-body {
  grid-template-columns: 2.5ch minmax(0, 1fr);
  column-gap: 6px;
  padding: 3px 10px 2px;
}

.tool-workflow-patch-line.is-note .tool-workflow-patch-line-body {
  grid-template-columns: 1fr;
}

.tool-workflow-patch-line.is-header .tool-workflow-patch-line-sign,
.tool-workflow-patch-line.is-meta .tool-workflow-patch-line-sign,
.tool-workflow-patch-line.is-note .tool-workflow-patch-line-sign {
  color: rgba(148, 163, 184, 0.88);
}

.tool-workflow-patch-line.is-note .tool-workflow-patch-line-no {
  opacity: 0.28;
}

.tool-workflow-patch-line.is-meta {
  color: var(--workflow-term-muted);
}

.tool-workflow-patch-line.is-header {
  color: rgba(148, 163, 184, 0.88);
}

.tool-workflow-patch-line.is-header .tool-workflow-patch-line-body {
  background: rgba(51, 65, 85, 0.22);
}

.tool-workflow-patch-line.is-meta .tool-workflow-patch-line-body {
  background: rgba(51, 65, 85, 0.42);
}

.tool-workflow-patch-line.is-note {
  color: var(--workflow-term-muted);
  font-weight: 600;
  background: rgba(148, 163, 184, 0.08);
}

.tool-workflow-patch-line.is-context {
  color: var(--workflow-term-text);
}

.tool-workflow-patch-line.is-context .tool-workflow-patch-line-body {
  background: rgba(15, 23, 42, 0.12);
}

.tool-workflow-patch-line.is-add {
  color: #bbf7d0;
}

.tool-workflow-patch-line.is-add .tool-workflow-patch-line-body {
  background: rgba(22, 101, 52, 0.44);
  border-left: 2px solid rgba(74, 222, 128, 0.62);
}

.tool-workflow-patch-line.is-delete {
  color: #fecaca;
}

.tool-workflow-patch-line.is-delete .tool-workflow-patch-line-body {
  background: rgba(127, 29, 29, 0.5);
  border-left: 2px solid rgba(248, 113, 113, 0.56);
}

.tool-workflow-patch-line.is-move,
.tool-workflow-patch-line.is-update {
  color: #bfdbfe;
}

.tool-workflow-patch-line.is-move .tool-workflow-patch-line-body,
.tool-workflow-patch-line.is-update .tool-workflow-patch-line-body {
  background: rgba(30, 64, 175, 0.36);
}

.tool-workflow-patch-line.is-error {
  color: #fecaca;
}

.tool-workflow-patch-line.is-error .tool-workflow-patch-line-body {
  background: rgba(153, 27, 27, 0.48);
}

@media (max-width: 640px) {
  .tool-workflow-patch-line {
    --patch-gutter-column-width: 48px;
    --patch-sign-width: 1.5ch;
  }

  .tool-workflow-patch-line-no {
    padding: 2px 4px 2px 8px;
  }

  .tool-workflow-patch-line-body {
    column-gap: 4px;
    padding: 2px 6px 2px 4px;
  }
}
</style>
