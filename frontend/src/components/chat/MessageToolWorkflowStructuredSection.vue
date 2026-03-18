<template>
  <div class="tool-workflow-structured" :class="`is-${view.variant}`">
    <div class="tool-workflow-structured-groups">
      <section
        v-for="group in view.groups"
        :key="group.key"
        class="tool-workflow-structured-group"
      >
        <header v-if="group.title" class="tool-workflow-structured-group-header">
          <span class="tool-workflow-structured-group-title">{{ group.title }}</span>
        </header>

        <div class="tool-workflow-structured-rows">
          <article
            v-for="row in group.rows"
            :key="row.key"
            :class="[
              'tool-workflow-structured-row',
              row.tone ? `is-${row.tone}` : '',
              row.body ? 'has-body' : '',
              row.mono ? 'is-mono' : ''
            ]"
          >
            <div class="tool-workflow-structured-row-head">
              <span class="tool-workflow-structured-row-title">{{ row.title }}</span>
              <span v-if="row.meta" class="tool-workflow-structured-row-meta">{{ row.meta }}</span>
            </div>
            <pre v-if="row.body" class="tool-workflow-structured-row-body">{{ row.body }}</pre>
          </article>
        </div>
      </section>
    </div>

    <pre v-if="body" class="tool-workflow-structured-footer">{{ body }}</pre>
  </div>
</template>

<script setup lang="ts">
import type { ToolWorkflowStructuredView } from './toolWorkflowTypes';

defineProps<{
  view: ToolWorkflowStructuredView;
  body?: string;
}>();
</script>

<style scoped>
.tool-workflow-structured {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.tool-workflow-structured-groups {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.tool-workflow-structured-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.tool-workflow-structured-group-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.tool-workflow-structured-group-title {
  color: var(--workflow-term-muted);
  font-size: 11px;
  font-weight: 700;
  line-height: 1.4;
  word-break: break-word;
}

.tool-workflow-structured-rows {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-structured-row {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px;
  border-radius: 10px;
  border: 1px solid var(--workflow-term-border);
  background: var(--workflow-term-bg-soft);
}

.tool-workflow-structured.is-list {
  gap: 6px;
}

.tool-workflow-structured.is-list .tool-workflow-structured-groups,
.tool-workflow-structured.is-list .tool-workflow-structured-group,
.tool-workflow-structured.is-list .tool-workflow-structured-rows {
  gap: 2px;
}

.tool-workflow-structured.is-list .tool-workflow-structured-row {
  gap: 0;
  padding: 2px 0;
  border: none;
  border-radius: 0;
  background: transparent;
}

.tool-workflow-structured.is-list .tool-workflow-structured-row-head {
  gap: 0;
  justify-content: flex-start;
}

.tool-workflow-structured-row.is-success {
  border-color: rgba(74, 222, 128, 0.24);
}

.tool-workflow-structured-row.is-warning {
  border-color: rgba(251, 191, 36, 0.24);
}

.tool-workflow-structured-row.is-danger {
  border-color: rgba(248, 113, 113, 0.24);
}

.tool-workflow-structured-row-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.tool-workflow-structured-row-title {
  min-width: 0;
  color: var(--workflow-term-text);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.45;
  word-break: break-word;
}

.tool-workflow-structured-row-meta {
  flex: 0 0 auto;
  color: var(--workflow-term-muted);
  font-size: 11px;
}

.tool-workflow-structured-row-body,
.tool-workflow-structured-footer {
  margin: 0;
  color: var(--workflow-term-code);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
}

.tool-workflow-structured-row.is-mono .tool-workflow-structured-row-title,
.tool-workflow-structured-row.is-mono .tool-workflow-structured-row-body,
.tool-workflow-structured-footer {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-structured-footer {
  padding: 10px;
  border-radius: 10px;
  border: 1px solid rgba(248, 113, 113, 0.22);
  background: rgba(127, 29, 29, 0.16);
  color: #fecaca;
}
</style>
