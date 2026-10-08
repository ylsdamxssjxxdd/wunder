<template>
  <main class="composer-b4-harness" data-testid="composer-b4-harness">
    <header class="composer-b4-toolbar">
      <button type="button" data-testid="composer-b4-loading-toggle" @click="loading = !loading">
        {{ loading ? 'stop -> send' : 'send -> stop' }}
      </button>
      <button type="button" data-testid="composer-b4-queue-reference" @click="queueReference">
        引用到聊天
      </button>
      <button type="button" data-testid="composer-b4-reset" @click="resetLog">Reset</button>
    </header>

    <pre class="composer-b4-log" data-testid="composer-b4-log">{{ logText }}</pre>

    <section class="messenger-view messenger-composer-scope chat-shell">
      <div class="messenger-chat-footer">
        <ChatComposer
          :loading="loading"
          send-key="enter"
          draft-key="composer-b4-harness"
          :approval-mode="approvalMode"
          :approval-mode-editable="true"
          :model-name="modelName"
          :apply-model="applyModel"
          :preset-questions="presetQuestions"
          @send="handleSend"
          @stop="log('stop')"
          @new-thread="log('new-thread')"
          @open-thread="log(`open-thread:${$event}`)"
          @update:approval-mode="handleApprovalMode"
          @update:reasoning-effort="(value) => log(`reasoning-effort:${value}`)"
        />
      </div>
    </section>
  </main>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';

import ChatComposer from '@/components/chat/ChatComposer.vue';
import { queueWorkspaceChatReference } from '@/views/messenger/workspace/workspaceChatReference';

defineOptions({ name: 'ComposerB4E2EHarnessView' });

const loading = ref(false);
const approvalMode = ref('full_auto');
const modelName = ref('harness-model-a');
const presetQuestions = ref(['总结当前线程', '解释这段补丁']);
const entries = ref<string[]>([]);

const log = (message: string) => {
  entries.value = [...entries.value.slice(-24), message];
};

const logText = computed(() => (entries.value.length ? entries.value.join('\n') : '(no events)'));

const handleSend = (payload: Record<string, unknown>) => {
  log(
    `send:${JSON.stringify({
      content: payload?.content,
      reasoningEffort: payload?.reasoningEffort,
      approvalMode: payload?.approvalMode,
      attachmentCount: Array.isArray(payload?.attachments) ? payload.attachments.length : 0
    })}`
  );
};

const handleApprovalMode = (value: string) => {
  approvalMode.value = value;
  log(`approval-mode:${value}`);
};

// Stands in for the controller chain so the popover -> apply -> label path is
// observable without touching a real agent record.
const applyModel = async (payload: { modelId: string; reasoningEffort: string }) => {
  log(`apply-model:${payload.modelId}@${payload.reasoningEffort}`);
  modelName.value = payload.modelId;
  return { ok: true, message: `applied:${payload.modelId}` };
};

const queueReference = () => {
  queueWorkspaceChatReference({ path: 'docs/harness-notes.md', name: 'harness-notes.md', isDir: false });
};

const resetLog = () => {
  entries.value = [];
};
</script>

<style scoped>
.composer-b4-harness {
  display: flex;
  flex-direction: column;
  gap: 8px;
  box-sizing: border-box;
  min-height: 100vh;
  padding: 12px 16px 24px;
  background: #fbfaf8;
}

.composer-b4-toolbar {
  display: flex;
  gap: 8px;
}

.composer-b4-toolbar button {
  padding: 6px 10px;
  border: 1px solid #e8e6e3;
  border-radius: 8px;
  background: #ffffff;
  font-size: 12px;
  cursor: pointer;
}

.composer-b4-log {
  max-height: 160px;
  margin: 0;
  padding: 8px;
  overflow: auto;
  border: 1px solid #e8e6e3;
  border-radius: 8px;
  background: #ffffff;
  color: #3d3d3d;
  font-size: 11px;
  white-space: pre-wrap;
}

/* The harness only needs the shell tokens; the two-column shell itself is out of scope. */
.composer-b4-harness .messenger-view {
  min-height: 320px;
}

.composer-b4-harness :deep(.messenger-chat-footer) {
  margin-top: auto;
}
</style>
