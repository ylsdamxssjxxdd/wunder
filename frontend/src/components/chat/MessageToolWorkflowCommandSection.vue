<template>
  <div class="tool-workflow-command-card">
    <div
      v-if="(view.exitCode !== null && view.showExitCode !== false) || view.metrics?.length"
      class="tool-workflow-command-head"
    >
      <span v-if="view.exitCode !== null && view.showExitCode !== false" class="tool-workflow-command-exit">
        exit {{ view.exitCode }}
      </span>
      <span v-else class="tool-workflow-command-exit">{{ statusLabel }}</span>
      <span v-if="view.metrics?.length" class="tool-workflow-command-meta">
        {{ formatMetrics(view.metrics) }}
      </span>
    </div>

    <div v-if="view.terminalText || view.streams?.length" class="tool-workflow-command-terminal">
      <div class="tool-workflow-command-terminal-head">
        <span class="tool-workflow-command-dot"></span>
        <span class="tool-workflow-command-dot"></span>
        <span class="tool-workflow-command-dot"></span>
      </div>
      <pre
        v-if="view.terminalText"
        class="tool-workflow-command-stream-body"
        :ref="(el) => bindStreamRef('stdout', el)"
        @scroll="(event) => onStreamBodyScroll?.('stdout', event)"
      >{{ view.terminalText }}</pre>
      <template v-else>
        <pre
          v-for="stream in terminalFallbackStreams"
          :key="stream.key"
          :class="['tool-workflow-command-stream-body', stream.tone ? `is-${stream.tone}` : '']"
          :ref="(el) => bindStreamRef(stream.key, el)"
          @scroll="(event) => onStreamBodyScroll?.(resolveStreamName(stream.key), event)"
        ><span v-if="terminalFallbackStreams.length > 1" class="tool-workflow-command-stream-prefix">[{{ stream.label }}]</span>{{ stream.body }}</pre>
      </template>
    </div>

    <section v-if="view.previewBody" class="tool-workflow-command-preview">
      <header class="tool-workflow-command-stream-head">
        <span class="tool-workflow-command-stream-label">preview</span>
      </header>
      <pre class="tool-workflow-command-stream-body">{{ view.previewBody }}</pre>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { ComponentPublicInstance } from 'vue';

import type { ToolWorkflowCommandView } from './toolWorkflowTypes';

type CommandStreamName = 'stdout' | 'stderr';

const props = defineProps<{
  view: ToolWorkflowCommandView;
  bindStreamBodyRef?: (stream: CommandStreamName, el: Element | ComponentPublicInstance | null) => void;
  onStreamBodyScroll?: (stream: CommandStreamName, event: Event) => void;
}>();

const statusLabel = computed(() => {
  const status = String(props.view.status || '').trim().toLowerCase();
  if (status === 'failed') return 'failed';
  if (status === 'completed') return 'done';
  return 'running';
});

const terminalFallbackStreams = computed(() => props.view.streams || []);

function resolveStreamName(key: string): CommandStreamName {
  return key.includes('stderr') ? 'stderr' : 'stdout';
}

function formatMetrics(metrics: ToolWorkflowCommandView['metrics']): string {
  return (metrics || [])
    .map((metric) => `${metric.label} ${metric.value}`.trim())
    .filter(Boolean)
    .join(' | ');
}

function bindStreamRef(key: string, el: Element | ComponentPublicInstance | null): void {
  props.bindStreamBodyRef?.(resolveStreamName(key), el);
}
</script>

<style scoped>
.tool-workflow-command-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-command-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.tool-workflow-command-exit,
.tool-workflow-command-meta {
  color: var(--workflow-term-muted);
  font-size: 11px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-command-exit {
  flex: 0 0 auto;
}

.tool-workflow-command-meta {
  min-width: 0;
  overflow: hidden;
  text-align: right;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tool-workflow-command-preview {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-command-terminal {
  display: flex;
  flex-direction: column;
  min-height: 96px;
  max-height: 220px;
  border: 1px solid var(--workflow-term-border);
  border-radius: 8px;
  background: rgba(3, 7, 18, 0.72);
  overflow: hidden;
}

.tool-workflow-command-terminal-head {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 24px;
  padding: 0 9px;
  border-bottom: 1px solid rgba(148, 163, 184, 0.16);
  background: rgba(15, 23, 42, 0.42);
}

.tool-workflow-command-dot {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.58);
}

.tool-workflow-command-stream-prefix {
  display: block;
  margin-bottom: 3px;
  color: var(--workflow-term-muted);
}

.tool-workflow-command-stream-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.tool-workflow-command-stream-label {
  color: var(--workflow-term-muted);
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.2px;
  text-transform: uppercase;
}

.tool-workflow-command-stream-body {
  margin: 0;
  max-height: 196px;
  min-height: 0;
  overflow-y: auto;
  padding: 8px 10px;
  color: var(--workflow-term-code);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
  scrollbar-color: var(--workflow-term-scroll-thumb) var(--workflow-term-scroll-track);
}

.tool-workflow-command-stream-body + .tool-workflow-command-stream-body {
  border-top: 1px solid rgba(148, 163, 184, 0.14);
}

.tool-workflow-command-stream-body.is-danger {
  color: #fecaca;
}

.tool-workflow-command-stream-body::-webkit-scrollbar {
  width: 8px;
  height: 8px;
}

.tool-workflow-command-stream-body::-webkit-scrollbar-track {
  background: var(--workflow-term-scroll-track);
}

.tool-workflow-command-stream-body::-webkit-scrollbar-thumb {
  background: var(--workflow-term-scroll-thumb);
  border-radius: 999px;
}
</style>

