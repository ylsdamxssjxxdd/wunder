<template>
  <div class="tool-workflow-command-card">
    <div class="tool-workflow-command-head">
      <span class="tool-workflow-command-shell">{{ view.shell }}</span>
      <span v-if="view.exitCode !== null && view.showExitCode !== false" class="tool-workflow-command-exit">
        exit {{ view.exitCode }}
      </span>
    </div>

    <pre v-if="view.command" class="tool-workflow-command-line">{{ view.command }}</pre>

    <div v-if="view.streams?.length" class="tool-workflow-command-streams">
      <section
        v-for="stream in view.streams"
        :key="stream.key"
        :class="['tool-workflow-command-stream', stream.tone ? `is-${stream.tone}` : '']"
      >
        <header class="tool-workflow-command-stream-head">
          <span class="tool-workflow-command-stream-label">{{ stream.label }}</span>
        </header>
        <pre
          class="tool-workflow-command-stream-body"
          :ref="(el) => bindStreamRef(stream.key, el)"
          @scroll="(event) => onStreamBodyScroll?.(resolveStreamName(stream.key), event)"
        >{{ stream.body }}</pre>
      </section>
    </div>

    <section v-else-if="view.terminalText" class="tool-workflow-command-stream">
      <pre
        class="tool-workflow-command-stream-body"
        :ref="(el) => bindStreamRef('stdout', el)"
        @scroll="(event) => onStreamBodyScroll?.('stdout', event)"
      >{{ view.terminalText }}</pre>
    </section>

    <section v-if="view.previewBody" class="tool-workflow-command-preview">
      <header class="tool-workflow-command-stream-head">
        <span class="tool-workflow-command-stream-label">preview</span>
      </header>
      <pre class="tool-workflow-command-stream-body">{{ view.previewBody }}</pre>
    </section>
  </div>
</template>

<script setup lang="ts">
import type { ComponentPublicInstance } from 'vue';

import type { ToolWorkflowCommandView } from './toolWorkflowTypes';

type CommandStreamName = 'stdout' | 'stderr';

const props = defineProps<{
  view: ToolWorkflowCommandView;
  bindStreamBodyRef?: (stream: CommandStreamName, el: Element | ComponentPublicInstance | null) => void;
  onStreamBodyScroll?: (stream: CommandStreamName, event: Event) => void;
}>();

function resolveStreamName(key: string): CommandStreamName {
  return key.includes('stderr') ? 'stderr' : 'stdout';
}

function bindStreamRef(key: string, el: Element | ComponentPublicInstance | null): void {
  props.bindStreamBodyRef?.(resolveStreamName(key), el);
}
</script>

<style scoped>
.tool-workflow-command-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.tool-workflow-command-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.tool-workflow-command-shell,
.tool-workflow-command-exit {
  color: var(--workflow-term-muted);
  font-size: 11px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-command-line {
  margin: 0;
  padding: 10px;
  border-radius: 10px;
  border: 1px solid var(--workflow-term-border);
  background: rgba(255, 255, 255, 0.03);
  color: var(--workflow-term-text);
  font-size: 12px;
  line-height: 1.45;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-command-streams,
.tool-workflow-command-preview {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-command-stream {
  display: flex;
  flex-direction: column;
  gap: 6px;
  border: 1px solid var(--workflow-term-border);
  border-radius: 10px;
  background: var(--workflow-term-bg-soft);
  padding: 10px;
}

.tool-workflow-command-stream.is-danger {
  border-color: rgba(248, 113, 113, 0.24);
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
  max-height: 260px;
  overflow: auto;
  color: var(--workflow-term-code);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
  scrollbar-color: var(--workflow-term-scroll-thumb) var(--workflow-term-scroll-track);
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
