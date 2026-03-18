<template>
  <div v-if="view" class="tool-workflow-compaction">
    <div class="tool-workflow-compaction-overview">
      <div class="tool-workflow-compaction-headline">{{ view.headline }}</div>
      <div class="tool-workflow-compaction-description">{{ view.description }}</div>
    </div>

    <div v-if="view.usageBar" :class="['tool-workflow-compaction-usage', `is-${view.usageBar.tone}`]">
      <div class="tool-workflow-compaction-usage-head">
        <span class="tool-workflow-compaction-usage-limit">{{ view.usageBar.limitLabel }}</span>
        <span class="tool-workflow-compaction-usage-hint">{{ view.usageBar.hint }}</span>
      </div>
      <div class="tool-workflow-compaction-usage-track">
        <span
          v-if="view.usageBar.beforeRatio !== null"
          class="tool-workflow-compaction-usage-fill is-before"
          :style="{ width: `${Math.max(view.usageBar.beforeRatio * 100, 6)}%` }"
        ></span>
        <span
          v-if="view.usageBar.afterRatio !== null"
          class="tool-workflow-compaction-usage-fill is-after"
          :style="{ width: `${Math.max(view.usageBar.afterRatio * 100, 6)}%` }"
        ></span>
      </div>
      <div class="tool-workflow-compaction-usage-legend">
        <span v-if="view.usageBar.beforeLabel" class="tool-workflow-compaction-usage-label is-before">
          {{ view.usageBar.beforeLabel }}
        </span>
        <span v-if="view.usageBar.afterLabel" class="tool-workflow-compaction-usage-label is-after">
          {{ view.usageBar.afterLabel }}
        </span>
      </div>
    </div>

    <div class="tool-workflow-compaction-stages">
      <div
        v-for="stage in view.stages"
        :key="stage.key"
        :class="['tool-workflow-compaction-stage', `is-${stage.state}`]"
      >
        <span class="tool-workflow-compaction-stage-dot" aria-hidden="true"></span>
        <div class="tool-workflow-compaction-stage-copy">
          <div class="tool-workflow-compaction-stage-label">{{ stage.label }}</div>
          <div class="tool-workflow-compaction-stage-detail">{{ stage.detail }}</div>
        </div>
      </div>
    </div>

    <div v-if="view.metrics.length" class="tool-workflow-compaction-metrics">
      <div
        v-for="metric in view.metrics"
        :key="metric.key"
        :class="['tool-workflow-compaction-metric', `is-${metric.tone}`]"
      >
        <span class="tool-workflow-compaction-metric-label">{{ metric.label }}</span>
        <span class="tool-workflow-compaction-metric-value">{{ metric.value }}</span>
      </div>
    </div>

    <div v-if="view.details.length" class="tool-workflow-compaction-details">
      <div v-for="detail in view.details" :key="detail.key" class="tool-workflow-compaction-detail-row">
        <span class="tool-workflow-compaction-detail-label">{{ detail.label }}</span>
        <span class="tool-workflow-compaction-detail-value">{{ detail.value }}</span>
      </div>
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

    <pre v-if="body.trim()" class="tool-workflow-compaction-extra">{{ body }}</pre>
  </div>
</template>

<script setup lang="ts">
import type { CompactionView } from '@/utils/chatCompactionUi';

defineProps<{
  view: CompactionView | null | undefined;
  body?: string;
}>();
</script>

<style scoped>
.tool-workflow-compaction {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.tool-workflow-compaction-overview {
  padding: 10px 12px;
  border-radius: 12px;
  border: 1px solid var(--workflow-term-border);
  background: linear-gradient(180deg, rgba(37, 99, 235, 0.12), rgba(15, 23, 42, 0.16));
}

.tool-workflow-compaction-headline {
  color: var(--workflow-term-text);
  font-size: 12px;
  font-weight: 700;
}

.tool-workflow-compaction-description {
  margin-top: 4px;
  color: var(--workflow-term-muted);
  font-size: 11px;
  line-height: 1.5;
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

.tool-workflow-compaction-usage-head,
.tool-workflow-compaction-usage-legend {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.tool-workflow-compaction-usage-limit,
.tool-workflow-compaction-usage-hint,
.tool-workflow-compaction-usage-label {
  font-size: 10px;
  line-height: 1.4;
}

.tool-workflow-compaction-usage-limit {
  color: var(--workflow-term-muted);
  font-weight: 700;
}

.tool-workflow-compaction-usage-hint {
  color: var(--workflow-term-text);
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

.tool-workflow-compaction-usage-label.is-before {
  color: #fecaca;
}

.tool-workflow-compaction-usage-label.is-after {
  color: #bfdbfe;
  text-align: right;
}

.tool-workflow-compaction-stages {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-compaction-stage {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 9px 10px;
  border-radius: 10px;
  border: 1px solid var(--workflow-term-border);
  background: rgba(15, 23, 42, 0.16);
}

.tool-workflow-compaction-stage-dot {
  width: 8px;
  height: 8px;
  margin-top: 5px;
  border-radius: 999px;
  flex: 0 0 auto;
  background: rgba(148, 163, 184, 0.72);
  box-shadow: 0 0 0 2px rgba(148, 163, 184, 0.12);
}

.tool-workflow-compaction-stage.is-done .tool-workflow-compaction-stage-dot {
  background: rgba(34, 197, 94, 0.96);
  box-shadow: 0 0 0 2px rgba(34, 197, 94, 0.18);
}

.tool-workflow-compaction-stage.is-active .tool-workflow-compaction-stage-dot {
  background: rgba(59, 130, 246, 0.98);
  box-shadow: 0 0 0 2px rgba(59, 130, 246, 0.18);
}

.tool-workflow-compaction-stage.is-warning .tool-workflow-compaction-stage-dot {
  background: rgba(245, 158, 11, 0.96);
  box-shadow: 0 0 0 2px rgba(245, 158, 11, 0.18);
}

.tool-workflow-compaction-stage-copy {
  min-width: 0;
  flex: 1 1 auto;
}

.tool-workflow-compaction-stage-label {
  color: var(--workflow-term-text);
  font-size: 11px;
  font-weight: 700;
}

.tool-workflow-compaction-stage-detail {
  margin-top: 2px;
  color: var(--workflow-term-muted);
  font-size: 11px;
  line-height: 1.45;
  white-space: pre-wrap;
  word-break: break-word;
}

.tool-workflow-compaction-metrics {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 8px;
}

.tool-workflow-compaction-metric {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  padding: 8px 10px;
  border-radius: 10px;
  border: 1px solid var(--workflow-term-border);
  background: var(--workflow-term-bg-soft);
}

.tool-workflow-compaction-metric.is-success {
  border-color: rgba(134, 239, 172, 0.34);
  background: rgba(22, 163, 74, 0.12);
}

.tool-workflow-compaction-metric.is-warning {
  border-color: rgba(252, 211, 77, 0.34);
  background: rgba(217, 119, 6, 0.12);
}

.tool-workflow-compaction-metric-label {
  color: var(--workflow-term-muted);
  font-size: 10px;
  font-weight: 700;
}

.tool-workflow-compaction-metric-value {
  color: var(--workflow-term-text);
  font-size: 11px;
  line-height: 1.45;
  word-break: break-word;
}

.tool-workflow-compaction-details {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px;
  border-radius: 10px;
  border: 1px solid var(--workflow-term-border);
  background: rgba(255, 255, 255, 0.02);
}

.tool-workflow-compaction-detail-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.tool-workflow-compaction-detail-label {
  flex: 0 0 auto;
  color: var(--workflow-term-muted);
  font-size: 11px;
}

.tool-workflow-compaction-detail-value {
  min-width: 0;
  color: var(--workflow-term-text);
  font-size: 11px;
  text-align: right;
  line-height: 1.45;
  word-break: break-word;
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

.tool-workflow-compaction-extra {
  margin: 0;
  padding: 10px;
  border-radius: 10px;
  border: 1px solid rgba(248, 113, 113, 0.24);
  background: rgba(127, 29, 29, 0.22);
  color: #fecaca;
  font-size: 11px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
