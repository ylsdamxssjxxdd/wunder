<template>
  <section class="tool-workflow-section" :class="{ 'is-empty': section.empty }">
    <div class="tool-workflow-section-header">
      <span class="tool-workflow-section-title">{{ section.title }}</span>
      <button
        v-if="section.copyText"
        class="tool-workflow-section-copy"
        type="button"
        :title="t('common.copy')"
        :aria-label="t('common.copy')"
        @click="handleCopySection"
      >
        <i class="fa-regular fa-copy" aria-hidden="true"></i>
      </button>
    </div>

    <div class="tool-workflow-section-body">
      <div
        v-if="section.kind === 'command' && section.commandView"
        class="tool-workflow-main tool-workflow-main--command"
        :class="{ 'is-empty': section.empty }"
      >
        <MessageToolWorkflowCommandSection
          :view="section.commandView"
          :bind-stream-body-ref="bindStreamBodyRef"
          :on-stream-body-scroll="onStreamBodyScroll"
        />
      </div>

      <div v-else-if="section.kind === 'patch'" class="tool-workflow-main tool-workflow-main--patch">
        <MessageToolWorkflowPatchSection
          v-if="section.patchView"
          :view="section.patchView"
        />
        <template v-else-if="section.patchLines.length">
          <div
            v-for="line in section.patchLines"
            :key="line.key"
            :class="['tool-workflow-patch-line', `is-${line.kind}`]"
          >
            {{ line.text }}
          </div>
        </template>
      </div>

      <MessageToolWorkflowCompactionSection
        v-else-if="section.kind === 'compaction' && section.compactionView"
        :view="section.compactionView"
        :body="section.body"
      />

      <MessageToolWorkflowStructuredSection
        v-else-if="section.kind === 'structured' && section.structuredView"
        :view="section.structuredView"
        :body="section.body"
      />

      <pre
        v-else-if="section.body || !section.summary"
        class="tool-workflow-main"
        :class="{ 'is-empty': section.empty }"
      >{{ section.body }}</pre>
    </div>
  </section>
</template>

<script setup lang="ts">
import type { ComponentPublicInstance } from 'vue';
import { ElMessage } from 'element-plus';

import { useI18n } from '@/i18n';
import { copyText } from '@/utils/clipboard';
import MessageToolWorkflowCommandSection from './MessageToolWorkflowCommandSection.vue';
import MessageToolWorkflowCompactionSection from './MessageToolWorkflowCompactionSection.vue';
import MessageToolWorkflowPatchSection from './MessageToolWorkflowPatchSection.vue';
import MessageToolWorkflowStructuredSection from './MessageToolWorkflowStructuredSection.vue';
import type { ToolWorkflowDetailSection } from './toolWorkflowTypes';

type CommandStreamName = 'stdout' | 'stderr';

const props = defineProps<{
  section: ToolWorkflowDetailSection;
  bindStreamBodyRef?: (stream: CommandStreamName, el: Element | ComponentPublicInstance | null) => void;
  onStreamBodyScroll?: (stream: CommandStreamName, event: Event) => void;
}>();

const { t } = useI18n();

const handleCopySection = async () => {
  if (!props.section.copyText) return;
  const copied = await copyText(props.section.copyText);
  if (copied) {
    ElMessage.success(t('chat.message.copySuccess'));
    return;
  }
  ElMessage.warning(t('chat.message.copyFailed'));
};
</script>

<style scoped>
.tool-workflow-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.tool-workflow-section + .tool-workflow-section {
  margin-top: 2px;
}

.tool-workflow-section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.tool-workflow-section-title {
  color: var(--workflow-term-muted);
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.2px;
}

.tool-workflow-section.is-empty .tool-workflow-section-title {
  opacity: 0.82;
}

.tool-workflow-section-copy {
  flex: 0 0 auto;
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 1px solid var(--workflow-term-border);
  border-radius: 7px;
  background: transparent;
  color: var(--workflow-term-muted);
  cursor: pointer;
  transition:
    color 0.16s ease,
    border-color 0.16s ease,
    background-color 0.16s ease;
}

.tool-workflow-section-copy:hover {
  color: var(--workflow-term-text);
  border-color: rgba(148, 163, 184, 0.45);
  background: rgba(148, 163, 184, 0.08);
}

.tool-workflow-section-body {
  min-width: 0;
}

.tool-workflow-main {
  margin: 0;
  padding: 10px;
  border-radius: 10px;
  border: 1px solid var(--workflow-term-border);
  background: var(--workflow-term-bg-soft);
  color: var(--workflow-term-text);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 320px;
  overflow: auto;
  scrollbar-color: var(--workflow-term-scroll-thumb) var(--workflow-term-scroll-track);
}

.tool-workflow-main.is-empty {
  border-style: dashed;
  color: var(--workflow-term-muted);
}

.tool-workflow-main::-webkit-scrollbar,
.tool-workflow-terminal-body::-webkit-scrollbar {
  width: 8px;
  height: 8px;
}

.tool-workflow-main::-webkit-scrollbar-track,
.tool-workflow-terminal-body::-webkit-scrollbar-track {
  background: var(--workflow-term-scroll-track);
}

.tool-workflow-main::-webkit-scrollbar-thumb,
.tool-workflow-terminal-body::-webkit-scrollbar-thumb {
  background: var(--workflow-term-scroll-thumb);
  border-radius: 999px;
}

.tool-workflow-main--command {
  white-space: normal;
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-terminal-head {
  color: var(--workflow-term-muted);
  font-size: 11px;
  letter-spacing: 0.2px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-terminal-body {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-size: 12px;
  line-height: 1.5;
  color: var(--workflow-term-code);
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
  max-height: 260px;
  overflow: auto;
  padding: 0;
  scrollbar-color: var(--workflow-term-scroll-thumb) var(--workflow-term-scroll-track);
}

.tool-workflow-terminal-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 6px;
}

.tool-workflow-terminal-exit-code {
  color: var(--workflow-term-muted);
  font-size: 11px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-main--patch {
  white-space: pre-wrap;
  word-break: break-word;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-patch-line {
  display: block;
  padding: 1px 4px;
  border-radius: 5px;
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
