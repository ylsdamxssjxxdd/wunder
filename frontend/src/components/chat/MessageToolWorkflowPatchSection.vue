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
            {{ line.text }}
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { ToolWorkflowPatchView } from './toolWorkflowTypes';

defineProps<{
  view: ToolWorkflowPatchView;
}>();
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
  gap: 2px;
}

.tool-workflow-patch-line {
  display: block;
  padding: 1px 4px;
  border-radius: 5px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
  white-space: pre-wrap;
  word-break: break-word;
}

.tool-workflow-patch-line.is-meta {
  color: var(--workflow-term-muted);
}

.tool-workflow-patch-line.is-note {
  color: var(--workflow-term-muted);
  font-weight: 600;
}

.tool-workflow-patch-line.is-add {
  color: #bbf7d0;
  background: rgba(22, 101, 52, 0.44);
  border-left: 2px solid rgba(74, 222, 128, 0.62);
}

.tool-workflow-patch-line.is-delete {
  color: #fecaca;
  background: rgba(127, 29, 29, 0.5);
  border-left: 2px solid rgba(248, 113, 113, 0.56);
}

.tool-workflow-patch-line.is-move,
.tool-workflow-patch-line.is-update {
  color: #bfdbfe;
  background: rgba(30, 64, 175, 0.36);
}

.tool-workflow-patch-line.is-error {
  color: #fecaca;
  background: rgba(153, 27, 27, 0.48);
}
</style>
