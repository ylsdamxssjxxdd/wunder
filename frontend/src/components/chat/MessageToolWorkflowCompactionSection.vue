<template>
  <div v-if="view" class="tool-workflow-compaction">
    <div v-if="view.usageBar" :class="['tool-workflow-compaction-usage', `is-${view.usageBar.tone}`]">
      <div class="tool-workflow-compaction-usage-track">
        <span
          v-if="view.usageBar.beforeBarRatio !== null"
          class="tool-workflow-compaction-usage-fill is-before"
          :style="{ width: `${Math.max(view.usageBar.beforeBarRatio * 100, 6)}%` }"
        ></span>
        <span
          v-if="view.usageBar.afterBarRatio !== null"
          class="tool-workflow-compaction-usage-fill is-after"
          :style="{ width: `${Math.max(view.usageBar.afterBarRatio * 100, 6)}%` }"
        ></span>
      </div>
      <div class="tool-workflow-compaction-usage-legend">
        <span v-if="view.usageBar.afterLabel" class="tool-workflow-compaction-usage-label is-after">
          {{ view.usageBar.afterLabel }}
        </span>
        <span v-if="view.usageBar.beforeLabel" class="tool-workflow-compaction-usage-label is-before">
          {{ view.usageBar.beforeLabel }}
        </span>
      </div>
    </div>

    <div v-if="view.outputs.length" class="tool-workflow-compaction-outputs">
      <section
        v-for="output in view.outputs"
        :key="output.key"
        :class="['tool-workflow-compaction-output', `is-${output.tone}`]"
      >
        <div class="tool-workflow-compaction-output-title">{{ output.title }}</div>
        <pre class="tool-workflow-compaction-output-body">{{ output.body }}</pre>
      </section>
    </div>
    <div v-else-if="view.outputEmpty" class="tool-workflow-compaction-output-empty">
      {{ view.outputEmpty }}
    </div>

    <div v-if="view.failure" class="tool-workflow-compaction-failure">
      <div class="tool-workflow-compaction-failure-title">{{ view.failure.title }}</div>
      <div class="tool-workflow-compaction-failure-description">{{ view.failure.description }}</div>
      <div class="tool-workflow-compaction-failure-actions">
        <span
          v-for="(suggestion, index) in view.failure.suggestions"
          :key="`${index}-${suggestion}`"
          class="tool-workflow-compaction-failure-chip"
        >
          {{ suggestion }}
        </span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { CompactionView } from '@/utils/chatCompactionUi';

defineProps<{
  view: CompactionView | null | undefined;
}>();
</script>

<style scoped>
.tool-workflow-compaction {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.tool-workflow-compaction-usage {
  padding: 10px;
  border-radius: 10px;
  border: 1px solid var(--workflow-term-border);
  background: rgba(15, 23, 42, 0.16);
}

.tool-workflow-compaction-usage.is-success {
  border-color: rgba(134, 239, 172, 0.3);
  background: rgba(22, 163, 74, 0.1);
}

.tool-workflow-compaction-usage.is-warning,
.tool-workflow-compaction-usage.is-danger {
  border-color: rgba(252, 211, 77, 0.3);
  background: rgba(217, 119, 6, 0.1);
}

.tool-workflow-compaction-usage.is-danger {
  border-color: rgba(248, 113, 113, 0.3);
  background: rgba(127, 29, 29, 0.2);
}

.tool-workflow-compaction-usage-legend {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.tool-workflow-compaction-usage-label {
  font-size: 10px;
  line-height: 1.4;
}

.tool-workflow-compaction-usage-track {
  position: relative;
  height: 10px;
  margin: 8px 0 6px;
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.12);
  overflow: hidden;
}

.tool-workflow-compaction-usage-fill {
  position: absolute;
  top: 0;
  left: 0;
  bottom: 0;
  border-radius: 999px;
}

.tool-workflow-compaction-usage-fill.is-before {
  background: rgba(248, 113, 113, 0.55);
}

.tool-workflow-compaction-usage-fill.is-after {
  background: rgba(59, 130, 246, 0.82);
}

.tool-workflow-compaction-usage-label {
  color: var(--workflow-term-muted);
}

.tool-workflow-compaction-usage-label.is-after {
  color: #bfdbfe;
}

.tool-workflow-compaction-usage-label.is-before {
  color: #fecaca;
  text-align: right;
}

.tool-workflow-compaction-outputs {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-compaction-output,
.tool-workflow-compaction-output-empty {
  padding: 10px 12px;
  border-radius: 10px;
  border: 1px solid var(--workflow-term-border);
  background: rgba(15, 23, 42, 0.16);
}

.tool-workflow-compaction-output.is-warning {
  border-color: rgba(245, 158, 11, 0.28);
  background: rgba(120, 53, 15, 0.16);
}

.tool-workflow-compaction-output-title {
  color: var(--workflow-term-text);
  font-size: 11px;
  font-weight: 700;
}

.tool-workflow-compaction-output-body,
.tool-workflow-compaction-output-empty {
  margin: 8px 0 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: 'JetBrains Mono', 'SFMono-Regular', Consolas, monospace;
  font-size: 11px;
  color: var(--workflow-term-muted);
  line-height: 1.45;
}

.tool-workflow-compaction-failure {
  padding: 10px;
  border-radius: 10px;
  border: 1px solid rgba(248, 113, 113, 0.28);
  background: rgba(127, 29, 29, 0.18);
}

.tool-workflow-compaction-failure-title {
  color: #fee2e2;
  font-size: 11px;
  font-weight: 700;
}

.tool-workflow-compaction-failure-description {
  margin-top: 4px;
  color: #fecaca;
  font-size: 11px;
  line-height: 1.5;
}

.tool-workflow-compaction-failure-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 8px;
}

.tool-workflow-compaction-failure-chip {
  border-radius: 999px;
  padding: 2px 8px;
  border: 1px solid rgba(254, 202, 202, 0.28);
  background: rgba(255, 255, 255, 0.06);
  color: #fee2e2;
  font-size: 10px;
  font-weight: 700;
}

</style>
